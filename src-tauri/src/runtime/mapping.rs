use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use base64::Engine;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::bridge::{
    CommandError, CommandResult, GenerateImagesRequest, GenerationMode, ReferenceAssetDto,
    ReferenceRole, ReferenceSourceType, parse_custom_options,
};
use crate::domain;
use crate::providers::capabilities;
use crate::providers::types as provider;
use crate::storage::{
    ProjectStore, StoredFile, atomic_write_new, sha256_bytes, strip_extended_length_prefix,
};

const MAX_REMOTE_INPUT_BYTES: u64 = 100 * 1024 * 1024;

#[derive(Debug, Clone)]
pub(crate) struct PreparedGenerationRequest {
    pub provider_request: provider::GenerationRequest,
    pub domain_request: domain::GenerationRequest,
    pub capability_snapshot: domain::ModelCapability,
    pub input_assets: Vec<domain::InputAsset>,
    pub mask_asset: Option<domain::InputAsset>,
}

pub(crate) async fn prepare_generation_request(
    project: &ProjectStore,
    request: &GenerateImagesRequest,
) -> CommandResult<PreparedGenerationRequest> {
    if request.draft.count == 0 || request.draft.count > 16 {
        return Err(CommandError::validation(
            "image count must be between 1 and 16",
        ));
    }
    if request.draft.model.trim().is_empty() {
        return Err(CommandError::validation("model is required"));
    }
    let prompt = compose_provider_prompt(request);
    if prompt.trim().is_empty() {
        return Err(CommandError::validation("prompt is required"));
    }
    let adapter_kind = request.provider.adapter_kind();
    let continues_conversation = (adapter_kind == provider::ProviderKind::Gemini
        && request
            .draft
            .previous_interaction_id
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty()))
        || (matches!(
            adapter_kind,
            provider::ProviderKind::OpenAi | provider::ProviderKind::OpenAiCompatible
        ) && request
            .draft
            .previous_response_id
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty()));
    if request.draft.mode == GenerationMode::Generate
        && !continues_conversation
        && (!request.draft.references.is_empty()
            || request
                .draft
                .mask_data_url
                .as_deref()
                .is_some_and(|mask| !mask.trim().is_empty()))
    {
        return Err(CommandError::validation(
            "generation mode cannot contain reference media or a mask; switch to edit mode or remove the inputs",
        ));
    }
    let mut provider_inputs = Vec::with_capacity(request.draft.references.len());
    let mut domain_inputs = Vec::with_capacity(request.draft.references.len());
    for reference in &request.draft.references {
        let (provider_asset, domain_asset) = prepare_reference(project, reference).await?;
        provider_inputs.push(provider_asset);
        domain_inputs.push(domain_asset);
    }
    validate_variation_input(request, &provider_inputs)?;
    let (provider_mask, domain_mask) = match request.draft.mask_data_url.as_deref() {
        Some(data) if !data.trim().is_empty() => {
            validate_mask(data, &provider_inputs)?;
            let reference = ReferenceAssetDto {
                id: Uuid::new_v4().to_string(),
                name: "mask.png".to_owned(),
                url: data.to_owned(),
                mime_type: "image/png".to_owned(),
                role: ReferenceRole::Source,
                source_type: Some(ReferenceSourceType::Base64),
                file_id: None,
            };
            let (provider_asset, mut domain_asset) = prepare_reference(project, &reference).await?;
            domain_asset.kind = domain::InputAssetKind::Mask;
            (Some(provider_asset), Some(domain_asset))
        }
        _ => (None, None),
    };

    let operation = if continues_conversation {
        provider::Operation::ConversationContinue
    } else {
        map_operation(request.draft.mode)
    };
    let execution = if request.draft.batch {
        provider::ExecutionMode::ProviderBatch
    } else if request.draft.background_task {
        provider::ExecutionMode::Background
    } else {
        provider::ExecutionMode::Realtime
    };
    let custom = parse_custom_options(&request.draft.custom_json)?;
    let output = provider_output(request, adapter_kind);
    let options = provider_options(request, adapter_kind, &custom)?;
    let overrides = request.provider.parsed_capability_overrides()?;
    let resolved_capability = capabilities::resolve_model(
        adapter_kind,
        &request.draft.model,
        overrides.get(&request.draft.model),
    )?
    .ok_or_else(|| {
        CommandError::validation(format!(
            "model {} has no capability entry or user override",
            request.draft.model
        ))
    })?;
    let capability_snapshot =
        map_resolved_capability(adapter_kind, &request.draft.model, &resolved_capability);
    let request_id = request
        .client_task_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let provider_model = if matches!(
        adapter_kind,
        provider::ProviderKind::OpenAi | provider::ProviderKind::OpenAiCompatible
    ) && options
        .openai
        .as_ref()
        .is_some_and(|options| options.api_surface == provider::OpenAiApiSurface::Responses)
    {
        non_empty_owned(request.draft.response_model.as_deref())
            .unwrap_or_else(|| "gpt-4.1".to_owned())
    } else {
        request.draft.model.clone()
    };
    let provider_request = provider::GenerationRequest {
        request_id,
        model: provider_model,
        resolved_capability: Some(Box::new(resolved_capability)),
        operation,
        execution,
        prompt: prompt.clone(),
        inputs: provider_inputs,
        mask: provider_mask,
        output,
        options,
    };

    let mut domain_request = domain::GenerationRequest::new(
        &request.project_id,
        &request.provider.id,
        &request.draft.model,
        &request.draft.prompt,
    );
    domain_request.id.clone_from(&provider_request.request_id);
    domain_request.operation = map_domain_operation(operation);
    domain_request.execution_mode = map_domain_execution(execution);
    domain_request.final_prompt = Some(prompt);
    domain_request.inputs.clone_from(&domain_inputs);
    domain_request.mask.clone_from(&domain_mask);
    domain_request.previous_interaction_id =
        non_empty_owned(request.draft.previous_interaction_id.as_deref());
    domain_request.output = domain_output(request);
    let persisted_custom = super::sanitize::sanitize_provider_json(&Value::Object(custom.clone()));
    domain_request.parameters = persisted_custom
        .as_object()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect();
    domain_request.context_ids.clone_from(&request.context_ids);
    domain_request.preset_id.clone_from(&request.preset_id);
    let mut frontend_draft = serde_json::to_value(&request.draft)?;
    if let Some(object) = frontend_draft.as_object_mut() {
        object.remove("references");
        object.remove("maskDataUrl");
        object.insert(
            "customJson".to_owned(),
            Value::String(
                serde_json::to_string(&crate::security::redact_json(&Value::Object(
                    domain_request.parameters.clone().into_iter().collect(),
                )))
                .unwrap_or_else(|_| "{}".to_owned()),
            ),
        );
        if let Some(manual_negative_prompt) = &request.manual_negative_prompt {
            object.insert(
                "negativePrompt".to_owned(),
                Value::String(manual_negative_prompt.clone()),
            );
        }
    }
    domain_request.metadata.insert(
        "frontendDraft".to_owned(),
        crate::security::redact_json(&frontend_draft),
    );
    domain_request.metadata.insert(
        "composedPromptBase".to_owned(),
        Value::String(request.composed_prompt.trim().to_owned()),
    );

    Ok(PreparedGenerationRequest {
        provider_request,
        domain_request,
        capability_snapshot,
        input_assets: domain_inputs,
        mask_asset: domain_mask,
    })
}

fn map_resolved_capability(
    kind: provider::ProviderKind,
    model: &str,
    capability: &capabilities::ModelCapability,
) -> domain::ModelCapability {
    let domain_kind = match kind {
        provider::ProviderKind::OpenAi => domain::ProviderKind::OpenAi,
        provider::ProviderKind::Xai => domain::ProviderKind::XAi,
        provider::ProviderKind::Gemini => domain::ProviderKind::Gemini,
        provider::ProviderKind::OpenAiCompatible => domain::ProviderKind::OpenAiCompatible,
    };
    let mut result = domain::ModelCapability::generic(domain_kind, model);
    result.schema_version = 1;
    result.display_name.clone_from(&capability.display_name);
    result.aliases = capability.aliases.iter().cloned().collect();
    result.operations = capability
        .operations
        .iter()
        .copied()
        .map(map_domain_operation)
        .collect();
    result.execution_modes = BTreeSet::from([domain::ExecutionMode::Realtime]);
    if capability.features.background {
        result
            .execution_modes
            .insert(domain::ExecutionMode::Background);
    }
    if capability.features.batch {
        result
            .execution_modes
            .insert(domain::ExecutionMode::ProviderBatch);
    }
    result.input_kinds = BTreeSet::from([domain::InputAssetKind::Image]);
    if capability.features.video_input {
        result.input_kinds.insert(domain::InputAssetKind::Video);
    }
    result.output_modalities = BTreeSet::from([domain::OutputModality::Image]);
    if capability.features.interleaved_text {
        result
            .output_modalities
            .insert(domain::OutputModality::Text);
        result
            .output_modalities
            .insert(domain::OutputModality::Thought);
        result
            .output_modalities
            .insert(domain::OutputModality::Citation);
    }
    result.max_prompt_chars = capability.prompt_max_chars;
    result.max_reference_images = capability
        .max_input_images
        .map(u16::from)
        .unwrap_or(u16::MAX);
    result.max_video_inputs = u16::from(capability.features.video_input);
    result.max_outputs = capability
        .output_count
        .as_ref()
        .map(|limit| limit.max.min(u32::from(u16::MAX)) as u16)
        .unwrap_or(1);
    result.sizes.supports_auto = capability.sizes.iter().any(|size| size == "auto");
    result.sizes.presets = capability.sizes.iter().cloned().collect();
    if let Some(custom) = &capability.custom_size {
        result.sizes.allow_custom = true;
        result.sizes.max_width = Some(custom.max_edge);
        result.sizes.max_height = Some(custom.max_edge);
        result.sizes.max_area = Some(custom.max_pixels);
        result.sizes.width_multiple_of = Some(custom.multiple_of);
        result.sizes.height_multiple_of = Some(custom.multiple_of);
        result.sizes.min_aspect_ratio = Some(custom.min_aspect_ratio);
        result.sizes.max_aspect_ratio = Some(custom.max_aspect_ratio);
    }
    result.aspect_ratios = capability.aspect_ratios.iter().cloned().collect();
    result.resolutions = capability.resolutions.iter().cloned().collect();
    result.qualities = capability.qualities.iter().cloned().collect();
    result.formats = capability
        .formats
        .iter()
        .filter_map(|format| map_domain_format(format))
        .collect();
    result.response_formats = capability
        .response_formats
        .iter()
        .filter_map(|format| match format.as_str() {
            "url" => Some(domain::ResponseFormat::Url),
            "b64_json" => Some(domain::ResponseFormat::Base64Json),
            _ => None,
        })
        .collect();
    result.backgrounds = capability
        .backgrounds
        .iter()
        .filter_map(|background| match background.as_str() {
            "auto" => Some(domain::Background::Auto),
            "opaque" => Some(domain::Background::Opaque),
            "transparent" => Some(domain::Background::Transparent),
            _ => None,
        })
        .collect();
    result.supports_mask = capability.features.mask;
    result.supports_streaming = capability.features.streaming;
    result.supports_partial_images = capability.features.streaming;
    result.supports_interleaved_output = capability.features.interleaved_text;
    result.supports_thinking = capability.features.thinking;
    result.supports_search = capability.features.google_search || capability.features.image_search;
    result.supports_provider_files = capability.features.file_inputs;
    result.supports_remote_storage = capability.features.file_outputs;
    result.allow_unknown_parameters = kind == provider::ProviderKind::OpenAiCompatible;
    result
}

pub(crate) fn build_batch_submission(
    request: &provider::GenerationRequest,
    desired_count: u16,
) -> CommandResult<provider::BatchSubmission> {
    let kind = provider_kind_from_options(request);
    let responses_batch = request
        .options
        .openai
        .as_ref()
        .is_some_and(|options| options.api_surface == provider::OpenAiApiSurface::Responses);
    let item_count = if kind == provider::ProviderKind::Gemini || responses_batch {
        desired_count
    } else {
        1
    };
    let endpoint = match kind {
        provider::ProviderKind::Gemini => format!("/models/{}:generateContent", request.model),
        provider::ProviderKind::OpenAi if responses_batch => "/v1/responses".to_owned(),
        _ if request.operation == provider::Operation::Edit => "/v1/images/edits".to_owned(),
        _ if request.operation == provider::Operation::Generate => {
            "/v1/images/generations".to_owned()
        }
        _ => {
            return Err(CommandError::validation(
                "this operation cannot be submitted as an inline provider batch",
            ));
        }
    };
    let mut requests = Vec::with_capacity(usize::from(item_count));
    for index in 0..item_count {
        requests.push(provider::BatchItem {
            key: format!("{}-{index}", request.request_id),
            endpoint: endpoint.clone(),
            body: batch_body(request, kind)?,
        });
    }
    Ok(provider::BatchSubmission {
        name: format!("ImageWorkbench {}", request.request_id),
        model: Some(request.model.clone()),
        requests,
        input_file_id: None,
        completion_window: Some("24h".to_owned()),
        metadata: Map::new(),
    })
}

fn provider_kind_from_options(request: &provider::GenerationRequest) -> provider::ProviderKind {
    if request.options.gemini.is_some() {
        provider::ProviderKind::Gemini
    } else if request.options.xai.is_some() {
        provider::ProviderKind::Xai
    } else {
        provider::ProviderKind::OpenAi
    }
}

fn provider_output(
    request: &GenerateImagesRequest,
    kind: provider::ProviderKind,
) -> provider::OutputSpec {
    let draft = &request.draft;
    let count = if kind == provider::ProviderKind::Gemini {
        1
    } else {
        u8::try_from(draft.count).unwrap_or(u8::MAX)
    };
    match kind {
        provider::ProviderKind::Xai => provider::OutputSpec {
            count,
            aspect_ratio: non_auto(&draft.aspect_ratio),
            resolution: non_auto(&draft.size),
            response_format: request.draft.response_format.as_deref().and_then(non_auto),
            ..provider::OutputSpec::default()
        },
        provider::ProviderKind::Gemini => provider::OutputSpec {
            count: 1,
            aspect_ratio: non_auto(&draft.aspect_ratio),
            resolution: non_auto(&draft.size),
            ..provider::OutputSpec::default()
        },
        provider::ProviderKind::OpenAi | provider::ProviderKind::OpenAiCompatible => {
            let format = non_auto(&draft.output_format).map(normalize_format);
            let compression = format
                .as_deref()
                .filter(|format| matches!(*format, "jpeg" | "webp"))
                .map(|_| draft.compression.clamp(1, 100));
            provider::OutputSpec {
                count,
                size: non_auto(&draft.size),
                aspect_ratio: None,
                resolution: None,
                quality: non_auto(&draft.quality),
                format,
                response_format: request.draft.response_format.as_deref().and_then(non_auto),
                background: non_auto(&draft.background),
                compression,
            }
        }
    }
}

fn provider_options(
    request: &GenerateImagesRequest,
    kind: provider::ProviderKind,
    custom: &Map<String, Value>,
) -> CommandResult<provider::ProviderOptions> {
    let mut generic_extra = custom.clone();
    if kind == provider::ProviderKind::OpenAiCompatible && !request.draft.seed.trim().is_empty() {
        let seed = request
            .draft
            .seed
            .trim()
            .parse::<i64>()
            .map_err(|_| CommandError::validation("seed must be an integer"))?;
        generic_extra.insert("seed".to_owned(), Value::from(seed));
    }
    let mut options = provider::ProviderOptions::default();
    match kind {
        provider::ProviderKind::OpenAi | provider::ProviderKind::OpenAiCompatible => {
            let previous_response_id =
                non_empty_owned(request.draft.previous_response_id.as_deref())
                    .or_else(|| custom_string(custom, "previous_response_id"));
            let image_generation_action =
                non_auto_owned(request.draft.image_generation_action.as_deref());
            let api_surface = if request.draft.background_task
                || request.draft.mode == GenerationMode::ConversationContinue
                || request.draft.use_responses_api == Some(true)
                || previous_response_id.is_some()
                || image_generation_action.is_some()
            {
                provider::OpenAiApiSurface::Responses
            } else {
                provider::OpenAiApiSurface::Images
            };
            let mut extra = generic_extra.clone();
            if api_surface == provider::OpenAiApiSurface::Responses
                && request.draft.service_tier != "standard"
            {
                extra.insert(
                    "service_tier".to_owned(),
                    Value::String(request.draft.service_tier.clone()),
                );
            }
            if api_surface == provider::OpenAiApiSurface::Responses
                && request.draft.store_interaction
            {
                extra.insert("store".to_owned(), Value::Bool(true));
            }
            options.openai = Some(provider::OpenAiOptions {
                api_surface,
                moderation: non_auto_owned(request.draft.moderation.as_deref()),
                style: request
                    .draft
                    .model
                    .eq_ignore_ascii_case("dall-e-3")
                    .then(|| non_auto_owned(request.draft.style.as_deref()))
                    .flatten(),
                user: non_empty_owned(request.draft.user.as_deref()),
                input_fidelity: non_auto(&request.draft.input_fidelity),
                image_model: (api_surface == provider::OpenAiApiSurface::Responses)
                    .then(|| request.draft.model.clone()),
                image_generation_action,
                previous_response_id,
                stream: request.draft.stream,
                partial_images: (request.draft.partial_images > 0)
                    .then_some(request.draft.partial_images),
                extra: extra.clone(),
            });
            options.extra = extra;
        }
        provider::ProviderKind::Xai => {
            options.xai = Some(provider::XaiOptions {
                storage: request.draft.xai_storage_filename.clone().map(|filename| {
                    provider::XaiStorageOptions {
                        filename,
                        expires_after: request.draft.xai_expires_after,
                        public_url: request.draft.xai_public_url.map(|enabled| {
                            request.draft.xai_public_url_expires_after.map_or(
                                provider::XaiPublicUrlOptions::Enabled(enabled),
                                |expires_after| provider::XaiPublicUrlOptions::Config {
                                    expires_after: Some(expires_after),
                                },
                            )
                        }),
                    }
                }),
                extra: generic_extra,
            });
        }
        provider::ProviderKind::Gemini => {
            let previous_interaction_id =
                non_empty_owned(request.draft.previous_interaction_id.as_deref())
                    .or_else(|| custom_string(custom, "previous_interaction_id"));
            let resume_interaction_id =
                non_empty_owned(request.draft.resume_interaction_id.as_deref())
                    .or_else(|| custom_string(custom, "resume_interaction_id"));
            let api_surface = if !request.draft.batch
                && (request.draft.use_interactions_api == Some(true)
                    || previous_interaction_id.is_some()
                    || resume_interaction_id.is_some()
                    || request.draft.background_task
                    || request.draft.store_interaction)
            {
                provider::GeminiApiSurface::Interactions
            } else {
                provider::GeminiApiSurface::GenerateContent
            };
            let thinking_level = non_auto(&request.draft.thinking_level);
            options.gemini = Some(provider::GeminiOptions {
                api_surface,
                response_modalities: if request.draft.include_text {
                    vec!["TEXT".to_owned(), "IMAGE".to_owned()]
                } else {
                    vec!["IMAGE".to_owned()]
                },
                include_thoughts: thinking_level.is_some(),
                thinking_level,
                temperature: Some(request.draft.temperature),
                top_p: Some(request.draft.top_p),
                google_search: request.draft.web_search,
                image_search: request.draft.image_search,
                stream: request.draft.stream,
                previous_interaction_id,
                resume_interaction_id,
                last_event_id: non_empty_owned(request.draft.last_event_id.as_deref())
                    .or_else(|| custom_string(custom, "last_event_id")),
                extra: generic_extra,
            });
        }
    }
    Ok(options)
}

fn domain_output(request: &GenerateImagesRequest) -> domain::OutputSpec {
    let draft = &request.draft;
    domain::OutputSpec {
        count: draft.count,
        size: if draft.size.eq_ignore_ascii_case("auto") {
            domain::ImageSize::Auto
        } else {
            domain::ImageSize::Preset {
                value: draft.size.clone(),
            }
        },
        aspect_ratio: non_auto(&draft.aspect_ratio),
        resolution: non_auto(&draft.size).filter(|size| size.ends_with('K') || size.ends_with('k')),
        quality: non_auto(&draft.quality),
        format: map_domain_format(&draft.output_format),
        compression: Some(draft.compression),
        background: match draft.background.as_str() {
            "auto" => Some(domain::Background::Auto),
            "opaque" => Some(domain::Background::Opaque),
            "transparent" => Some(domain::Background::Transparent),
            _ => None,
        },
        style: None,
        moderation: None,
        response_format: None,
        modalities: if draft.include_text {
            BTreeSet::from([domain::OutputModality::Image, domain::OutputModality::Text])
        } else {
            BTreeSet::from([domain::OutputModality::Image])
        },
        remote_storage: None,
        ttl_seconds: None,
    }
}

async fn prepare_reference(
    project: &ProjectStore,
    reference: &ReferenceAssetDto,
) -> CommandResult<(provider::InputAsset, domain::InputAsset)> {
    if reference.source_type == Some(ReferenceSourceType::FileId)
        || reference.url.starts_with("provider-file:")
    {
        let file_id = reference
            .file_id
            .as_deref()
            .or_else(|| reference.url.strip_prefix("provider-file:"))
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| CommandError::validation("reference.fileId is required"))?;
        let kind = if reference.role == ReferenceRole::Video
            || reference.mime_type.starts_with("video/")
        {
            domain::InputAssetKind::Video
        } else {
            domain::InputAssetKind::Image
        };
        return Ok((
            provider::InputAsset::FileId {
                id: file_id.to_owned(),
                mime_type: Some(reference.mime_type.clone()),
                label: Some(reference.name.clone()),
            },
            domain::InputAsset {
                id: reference.id.clone(),
                kind,
                source: domain::InputSource::ProviderFile {
                    file_id: file_id.to_owned(),
                },
                mime_type: Some(reference.mime_type.clone()),
                role: Some(reference_role(reference.role).to_owned()),
                label: Some(reference.name.clone()),
                sha256: None,
                size_bytes: None,
                metadata: BTreeMap::new(),
            },
        ));
    }
    let (bytes, mime_type) = if reference.url.starts_with("data:") {
        decode_data_url(&reference.url, &reference.mime_type)?
    } else if reference.url.starts_with("http://") || reference.url.starts_with("https://") {
        download_remote_input(&reference.url, &reference.mime_type).await?
    } else {
        let path = local_reference_path(project, &reference.url)?;
        (std::fs::read(path)?, reference.mime_type.clone())
    };
    let stored = store_input(project, &bytes, &mime_type)?;
    let provider_asset = provider::InputAsset::LocalFile {
        path: stored.path.clone(),
        mime_type: mime_type.clone(),
        label: Some(reference.name.clone()),
    };
    let relative = stored
        .path
        .strip_prefix(project.layout().root())
        .map_err(|_| CommandError::validation("stored input escaped the project root"))?
        .to_owned();
    let kind = if reference.role == ReferenceRole::Video || mime_type.starts_with("video/") {
        domain::InputAssetKind::Video
    } else {
        domain::InputAssetKind::Image
    };
    let domain_asset = domain::InputAsset {
        id: reference.id.clone(),
        kind,
        source: domain::InputSource::LocalPath { path: relative },
        mime_type: Some(mime_type),
        role: Some(reference_role(reference.role).to_owned()),
        label: Some(reference.name.clone()),
        sha256: Some(stored.sha256),
        size_bytes: Some(stored.size_bytes),
        metadata: image::load_from_memory(&bytes)
            .map(|image| {
                BTreeMap::from([
                    ("width".to_owned(), Value::from(image.width())),
                    ("height".to_owned(), Value::from(image.height())),
                ])
            })
            .unwrap_or_default(),
    };
    Ok((provider_asset, domain_asset))
}

fn store_input(project: &ProjectStore, bytes: &[u8], mime_type: &str) -> CommandResult<StoredFile> {
    let hash = sha256_bytes(bytes);
    let extension = extension_for_mime(mime_type);
    let path = project
        .layout()
        .input_directory()
        .join(format!("{hash}.{extension}"));
    if path.is_file() {
        return Ok(StoredFile {
            path,
            sha256: hash,
            size_bytes: bytes.len() as u64,
        });
    }
    atomic_write_new(path, bytes).map_err(Into::into)
}

async fn download_remote_input(url: &str, fallback_mime: &str) -> CommandResult<(Vec<u8>, String)> {
    let response = reqwest::Client::new()
        .get(url)
        .send()
        .await
        .map_err(|error| CommandError::new("input_download", error.to_string()))?;
    if !response.status().is_success() {
        return Err(CommandError::new(
            "input_download",
            format!("input URL returned HTTP {}", response.status()),
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_REMOTE_INPUT_BYTES)
    {
        return Err(CommandError::validation("remote input exceeds 100 MB"));
    }
    let mime = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.split(';').next().unwrap_or(value).to_owned())
        .unwrap_or_else(|| fallback_mime.to_owned());
    let bytes = response
        .bytes()
        .await
        .map_err(|error| CommandError::new("input_download", error.to_string()))?;
    if bytes.len() as u64 > MAX_REMOTE_INPUT_BYTES {
        return Err(CommandError::validation("remote input exceeds 100 MB"));
    }
    Ok((bytes.to_vec(), mime))
}

fn local_reference_path(project: &ProjectStore, value: &str) -> CommandResult<PathBuf> {
    let path = if value.starts_with("file:") {
        url::Url::parse(value)
            .map_err(|error| CommandError::validation(error.to_string()))?
            .to_file_path()
            .map_err(|_| CommandError::validation("invalid file URL"))?
    } else {
        let path = PathBuf::from(value);
        if path.is_absolute() {
            path
        } else {
            project.layout().root().join(path)
        }
    };
    let canonical = path.canonicalize().map(strip_extended_length_prefix)?;
    if !canonical.is_file() {
        return Err(CommandError::validation(
            "reference local path must point to a file",
        ));
    }
    if !canonical.starts_with(project.layout().root()) {
        return Err(CommandError::validation(
            "reference path is outside the project; import it first",
        ));
    }
    Ok(canonical)
}

fn validate_mask(mask_data_url: &str, inputs: &[provider::InputAsset]) -> CommandResult<()> {
    let (mask_bytes, mime) = decode_data_url(mask_data_url, "image/png")?;
    if !mime.eq_ignore_ascii_case("image/png")
        || image::guess_format(&mask_bytes).ok() != Some(image::ImageFormat::Png)
    {
        return Err(CommandError::validation("mask must be a PNG image"));
    }
    if mask_bytes.len() > 50 * 1024 * 1024 {
        return Err(CommandError::validation("mask exceeds the 50 MB limit"));
    }
    let mask = image::load_from_memory(&mask_bytes)
        .map_err(|error| CommandError::validation(format!("mask is invalid: {error}")))?;
    if !mask.color().has_alpha() {
        return Err(CommandError::validation(
            "mask PNG must contain an alpha channel",
        ));
    }
    let source = inputs
        .first()
        .ok_or_else(|| CommandError::validation("mask requires a source image"))?;
    let source_bytes = match source {
        provider::InputAsset::LocalFile { path, .. } => std::fs::read(path)?,
        provider::InputAsset::Base64 { data, .. } => base64::engine::general_purpose::STANDARD
            .decode(data.split_once(',').map(|(_, value)| value).unwrap_or(data))
            .map_err(|error| CommandError::validation(format!("reference is invalid: {error}")))?,
        provider::InputAsset::Url { .. } | provider::InputAsset::FileId { .. } => {
            return Err(CommandError::validation(
                "mask dimensions cannot be verified against a remote source; import the source image locally",
            ));
        }
    };
    let source = image::load_from_memory(&source_bytes).map_err(|error| {
        CommandError::validation(format!("references[0] is not a valid image: {error}"))
    })?;
    if mask.width() != source.width() || mask.height() != source.height() {
        return Err(CommandError::validation(format!(
            "mask dimensions {}x{} must match references[0] dimensions {}x{}",
            mask.width(),
            mask.height(),
            source.width(),
            source.height()
        )));
    }
    Ok(())
}

fn validate_variation_input(
    request: &GenerateImagesRequest,
    inputs: &[provider::InputAsset],
) -> CommandResult<()> {
    if request.draft.mode != GenerationMode::Variation || request.draft.model != "dall-e-2" {
        return Ok(());
    }
    if inputs.len() != 1 {
        return Err(CommandError::validation(
            "DALL-E 2 variation requires exactly one reference image",
        ));
    }
    let (bytes, mime_type) = match &inputs[0] {
        provider::InputAsset::LocalFile {
            path, mime_type, ..
        } => (std::fs::read(path)?, mime_type.as_str()),
        provider::InputAsset::Base64 {
            data, mime_type, ..
        } => (
            base64::engine::general_purpose::STANDARD
                .decode(data.split_once(',').map(|(_, value)| value).unwrap_or(data))
                .map_err(|error| {
                    CommandError::validation(format!("references[0] is invalid: {error}"))
                })?,
            mime_type.as_str(),
        ),
        provider::InputAsset::Url { .. } | provider::InputAsset::FileId { .. } => {
            return Err(CommandError::validation(
                "DALL-E 2 variation requires a locally imported PNG",
            ));
        }
    };
    if bytes.len() > 4 * 1024 * 1024 {
        return Err(CommandError::validation(
            "DALL-E 2 variation reference exceeds 4 MB",
        ));
    }
    if !mime_type.eq_ignore_ascii_case("image/png")
        || image::guess_format(&bytes).ok() != Some(image::ImageFormat::Png)
    {
        return Err(CommandError::validation(
            "DALL-E 2 variation reference must be PNG",
        ));
    }
    let image = image::load_from_memory(&bytes)
        .map_err(|error| CommandError::validation(format!("references[0] is invalid: {error}")))?;
    if image.width() != image.height() {
        return Err(CommandError::validation(
            "DALL-E 2 variation reference must be square",
        ));
    }
    Ok(())
}

fn decode_data_url(value: &str, fallback_mime: &str) -> CommandResult<(Vec<u8>, String)> {
    let (metadata, encoded) = value
        .split_once(',')
        .ok_or_else(|| CommandError::validation("invalid data URL"))?;
    if !metadata.ends_with(";base64") {
        return Err(CommandError::validation(
            "only base64 data URLs are supported",
        ));
    }
    let mime = metadata
        .strip_prefix("data:")
        .and_then(|value| value.strip_suffix(";base64"))
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback_mime)
        .to_owned();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| CommandError::validation(format!("invalid base64 input: {error}")))?;
    Ok((bytes, mime))
}

fn batch_body(
    request: &provider::GenerationRequest,
    kind: provider::ProviderKind,
) -> CommandResult<Value> {
    match kind {
        provider::ProviderKind::Gemini => gemini_batch_body(request),
        provider::ProviderKind::Xai => xai_batch_body(request),
        provider::ProviderKind::OpenAi | provider::ProviderKind::OpenAiCompatible => {
            openai_batch_body(request)
        }
    }
}

fn openai_batch_body(request: &provider::GenerationRequest) -> CommandResult<Value> {
    let options = request.options.openai.clone().unwrap_or_default();
    if options.api_surface == provider::OpenAiApiSurface::Responses {
        let mut content = vec![json!({
            "type": "input_text",
            "text": request.prompt,
        })];
        for input in &request.inputs {
            content.push(match input {
                provider::InputAsset::FileId { id, .. } => {
                    json!({ "type": "input_image", "file_id": id })
                }
                _ => {
                    let reference = provider_input_reference(input)?;
                    json!({
                        "type": "input_image",
                        "image_url": reference.get("url").cloned().unwrap_or(Value::Null),
                    })
                }
            });
        }
        let mut tool = Map::from_iter([(
            "type".to_owned(),
            Value::String("image_generation".to_owned()),
        )]);
        insert_optional(&mut tool, "model", options.image_model.as_deref());
        insert_optional(&mut tool, "size", request.output.size.as_deref());
        insert_optional(&mut tool, "quality", request.output.quality.as_deref());
        insert_optional(&mut tool, "output_format", request.output.format.as_deref());
        insert_optional(
            &mut tool,
            "input_fidelity",
            options.input_fidelity.as_deref(),
        );
        insert_optional(
            &mut tool,
            "action",
            options.image_generation_action.as_deref(),
        );
        return Ok(json!({
            "model": request.model,
            "input": [{ "role": "user", "content": content }],
            "tools": [Value::Object(tool)],
            "tool_choice": { "type": "image_generation" },
        }));
    }
    let mut body = Map::from_iter([
        ("model".to_owned(), Value::String(request.model.clone())),
        ("prompt".to_owned(), Value::String(request.prompt.clone())),
        ("n".to_owned(), Value::from(request.output.count)),
    ]);
    insert_optional(&mut body, "size", request.output.size.as_deref());
    insert_optional(&mut body, "quality", request.output.quality.as_deref());
    insert_optional(&mut body, "output_format", request.output.format.as_deref());
    insert_optional(
        &mut body,
        "background",
        request.output.background.as_deref(),
    );
    if let Some(compression) = request.output.compression {
        body.insert("output_compression".to_owned(), Value::from(compression));
    }
    if request.operation == provider::Operation::Edit {
        body.insert(
            "images".to_owned(),
            Value::Array(
                request
                    .inputs
                    .iter()
                    .map(provider_input_reference)
                    .collect::<CommandResult<Vec<_>>>()?,
            ),
        );
    }
    Ok(Value::Object(body))
}

fn xai_batch_body(request: &provider::GenerationRequest) -> CommandResult<Value> {
    let mut body = Map::from_iter([
        ("model".to_owned(), Value::String(request.model.clone())),
        ("prompt".to_owned(), Value::String(request.prompt.clone())),
        ("n".to_owned(), Value::from(request.output.count)),
    ]);
    insert_optional(
        &mut body,
        "aspect_ratio",
        request.output.aspect_ratio.as_deref(),
    );
    insert_optional(
        &mut body,
        "resolution",
        request.output.resolution.as_deref(),
    );
    if request.operation == provider::Operation::Edit {
        let images = request
            .inputs
            .iter()
            .map(provider_input_reference)
            .collect::<CommandResult<Vec<_>>>()?;
        if images.len() == 1 {
            body.insert("image".to_owned(), images.into_iter().next().unwrap());
        } else {
            body.insert("images".to_owned(), Value::Array(images));
        }
    }
    Ok(Value::Object(body))
}

fn gemini_batch_body(request: &provider::GenerationRequest) -> CommandResult<Value> {
    let mut parts = vec![json!({ "text": request.prompt })];
    for input in &request.inputs {
        let (mime_type, data) = provider_input_base64(input)?;
        parts.push(json!({
            "inlineData": { "mimeType": mime_type, "data": data }
        }));
    }
    let mut image_config = Map::new();
    insert_optional(
        &mut image_config,
        "aspectRatio",
        request.output.aspect_ratio.as_deref(),
    );
    insert_optional(
        &mut image_config,
        "imageSize",
        request.output.resolution.as_deref(),
    );
    let options = request.options.gemini.clone().unwrap_or_default();
    Ok(json!({
        "contents": [{ "role": "user", "parts": parts }],
        "generationConfig": {
            "responseModalities": options.response_modalities,
            "imageConfig": image_config
        }
    }))
}

fn provider_input_reference(input: &provider::InputAsset) -> CommandResult<Value> {
    match input {
        provider::InputAsset::FileId { id, .. } => Ok(json!({ "file_id": id })),
        _ => {
            let (mime_type, data) = provider_input_base64(input)?;
            Ok(json!({
                "url": format!("data:{mime_type};base64,{data}"),
                "type": "image_url"
            }))
        }
    }
}

fn provider_input_base64(input: &provider::InputAsset) -> CommandResult<(String, String)> {
    match input {
        provider::InputAsset::LocalFile {
            path, mime_type, ..
        } => Ok((
            mime_type.clone(),
            base64::engine::general_purpose::STANDARD.encode(std::fs::read(path)?),
        )),
        provider::InputAsset::Base64 {
            data, mime_type, ..
        } => Ok((
            mime_type.clone(),
            data.split_once(',')
                .map(|(_, encoded)| encoded)
                .unwrap_or(data)
                .to_owned(),
        )),
        provider::InputAsset::Url { .. } => Err(CommandError::validation(
            "batch inputs must be copied into the project before submission",
        )),
        provider::InputAsset::FileId { .. } => Err(CommandError::validation(
            "provider file IDs require provider-specific batch JSON",
        )),
    }
}

fn compose_provider_prompt(request: &GenerateImagesRequest) -> String {
    let mut prompt = request.composed_prompt.trim().to_owned();
    if !request.draft.negative_prompt.trim().is_empty() {
        if !prompt.is_empty() {
            prompt.push_str("\n\n");
        }
        prompt.push_str("Avoid: ");
        prompt.push_str(request.draft.negative_prompt.trim());
    }
    prompt
}

fn map_operation(mode: GenerationMode) -> provider::Operation {
    match mode {
        GenerationMode::Generate => provider::Operation::Generate,
        GenerationMode::Edit | GenerationMode::Mask => provider::Operation::Edit,
        GenerationMode::Video => provider::Operation::VideoReferenceToImage,
        GenerationMode::Variation => provider::Operation::Variation,
        GenerationMode::ConversationContinue => provider::Operation::ConversationContinue,
    }
}

fn map_domain_operation(operation: provider::Operation) -> domain::Operation {
    match operation {
        provider::Operation::Generate => domain::Operation::Generate,
        provider::Operation::Edit => domain::Operation::Edit,
        provider::Operation::Variation => domain::Operation::Variation,
        provider::Operation::ConversationContinue => domain::Operation::ConversationContinue,
        provider::Operation::VideoReferenceToImage => domain::Operation::VideoReferenceToImage,
    }
}

fn map_domain_execution(execution: provider::ExecutionMode) -> domain::ExecutionMode {
    match execution {
        provider::ExecutionMode::Realtime => domain::ExecutionMode::Realtime,
        provider::ExecutionMode::Background => domain::ExecutionMode::Background,
        provider::ExecutionMode::ProviderBatch => domain::ExecutionMode::ProviderBatch,
    }
}

fn map_domain_format(format: &str) -> Option<domain::ImageFormat> {
    match format.to_ascii_lowercase().as_str() {
        "png" | "image/png" => Some(domain::ImageFormat::Png),
        "jpg" | "jpeg" | "image/jpeg" => Some(domain::ImageFormat::Jpeg),
        "webp" | "image/webp" => Some(domain::ImageFormat::Webp),
        _ => None,
    }
}

fn non_auto(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && !value.eq_ignore_ascii_case("auto")).then(|| value.to_owned())
}

fn non_empty_owned(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn non_auto_owned(value: Option<&str>) -> Option<String> {
    non_empty_owned(value).filter(|value| !value.eq_ignore_ascii_case("auto"))
}

fn normalize_format(format: String) -> String {
    if format.eq_ignore_ascii_case("jpg") {
        "jpeg".to_owned()
    } else {
        format.to_ascii_lowercase()
    }
}

fn reference_role(role: ReferenceRole) -> &'static str {
    match role {
        ReferenceRole::Object => "object",
        ReferenceRole::Character => "character",
        ReferenceRole::Style => "style",
        ReferenceRole::Source => "source",
        ReferenceRole::Video => "video",
    }
}

fn extension_for_mime(mime_type: &str) -> &'static str {
    match mime_type.to_ascii_lowercase().as_str() {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/webp" => "webp",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        _ => "png",
    }
}

fn insert_optional(body: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        body.insert(key.to_owned(), Value::String(value.to_owned()));
    }
}

fn custom_string(custom: &Map<String, Value>, key: &str) -> Option<String> {
    non_empty_owned(custom.get(key).and_then(Value::as_str))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::bridge::{ApiMode, FrontendProviderKind, GenerationDraftDto, ProviderProfileDto};

    fn request() -> GenerateImagesRequest {
        GenerateImagesRequest {
            client_task_id: None,
            project_id: "project".to_owned(),
            storage_path: "project".to_owned(),
            provider: ProviderProfileDto {
                id: "provider".to_owned(),
                name: "Gemini".to_owned(),
                kind: FrontendProviderKind::Gemini,
                base_url: "https://generativelanguage.googleapis.com/v1beta".to_owned(),
                api_key: String::new(),
                has_stored_secret: None,
                api_mode: ApiMode::Native,
                enabled: true,
                models: vec![],
                discovered_models: vec![],
                api_version: Some("v1beta".to_owned()),
                organization: None,
                project_id: None,
                custom_header: None,
                last_synced_at: None,
                timeout_seconds: None,
                timeout_ms: None,
                proxy_url: None,
                auth_scheme: None,
                auth_header_name: None,
                auth_prefix: None,
                auth_query_name: None,
                custom_headers: vec![],
                models_path: None,
                compatibility_json: None,
                capability_overrides_json: None,
                default_stream: None,
            },
            manual_negative_prompt: None,
            draft: GenerationDraftDto {
                provider_id: "provider".to_owned(),
                model: "gemini-3.1-flash-image".to_owned(),
                mode: GenerationMode::Generate,
                prompt: "city".to_owned(),
                negative_prompt: String::new(),
                aspect_ratio: "1:1".to_owned(),
                size: "1K".to_owned(),
                quality: "auto".to_owned(),
                count: 3,
                output_format: "auto".to_owned(),
                response_format: None,
                background: "auto".to_owned(),
                compression: 90,
                references: vec![],
                mask_data_url: None,
                seed: String::new(),
                input_fidelity: "auto".to_owned(),
                thinking_level: "minimal".to_owned(),
                web_search: false,
                image_search: false,
                include_text: false,
                stream: false,
                background_task: false,
                batch: false,
                partial_images: 0,
                service_tier: "standard".to_owned(),
                temperature: 1.0,
                top_p: 1.0,
                store_interaction: false,
                custom_json: String::new(),
                previous_response_id: None,
                previous_interaction_id: None,
                use_responses_api: None,
                use_interactions_api: None,
                response_model: None,
                image_generation_action: None,
                moderation: None,
                style: None,
                user: None,
                xai_storage_filename: None,
                xai_expires_after: None,
                xai_public_url: None,
                xai_public_url_expires_after: None,
                resume_interaction_id: None,
                last_event_id: None,
                output_filename: String::new(),
                flat_output: false,
            },
            composed_prompt: "city".to_owned(),
            context_ids: vec![],
            preset_id: None,
            context_snapshot: vec![],
            preset_snapshot: None,
        }
    }

    #[tokio::test]
    async fn generation_mode_rejects_reference_media_before_provider_execution() {
        let directory = tempfile::tempdir().unwrap();
        let project = ProjectStore::create(directory.path().join("project"), "Project")
            .await
            .unwrap();
        let mut request = request();
        request.draft.references.push(ReferenceAssetDto {
            id: "reference".to_owned(),
            name: "reference.png".to_owned(),
            url: "assets/inputs/reference.png".to_owned(),
            mime_type: "image/png".to_owned(),
            role: ReferenceRole::Source,
            source_type: Some(ReferenceSourceType::Local),
            file_id: None,
        });

        let error = prepare_generation_request(&project, &request)
            .await
            .unwrap_err();

        assert_eq!(error.code, "validation");
        assert!(error.message.contains("switch to edit mode"));
    }

    #[tokio::test]
    async fn conversation_continuation_can_include_reference_media() {
        let directory = tempfile::tempdir().unwrap();
        let project = ProjectStore::create(directory.path().join("project"), "Project")
            .await
            .unwrap();
        let mut image_bytes = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::new(8, 8))
            .write_to(&mut image_bytes, image::ImageFormat::Png)
            .unwrap();
        let mut request = request();
        request.draft.previous_interaction_id = Some("interaction-1".to_owned());
        request.draft.references.push(ReferenceAssetDto {
            id: "reference".to_owned(),
            name: "reference.png".to_owned(),
            url: format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(image_bytes.into_inner())
            ),
            mime_type: "image/png".to_owned(),
            role: ReferenceRole::Source,
            source_type: Some(ReferenceSourceType::Base64),
            file_id: None,
        });

        let prepared = prepare_generation_request(&project, &request)
            .await
            .unwrap();

        assert_eq!(
            prepared.provider_request.operation,
            provider::Operation::ConversationContinue
        );
        assert_eq!(prepared.provider_request.inputs.len(), 1);
    }

    #[tokio::test]
    async fn edit_mode_reads_an_external_image_after_it_is_imported_into_the_project() {
        let directory = tempfile::tempdir().unwrap();
        let external_source = directory.path().join("reference.png");
        image::DynamicImage::ImageRgba8(image::RgbaImage::new(8, 8))
            .save(&external_source)
            .unwrap();
        let project = ProjectStore::create(directory.path().join("project"), "Project")
            .await
            .unwrap();
        let imported =
            crate::storage::import_input_file(project.layout(), &external_source).unwrap();
        let relative = imported
            .path
            .strip_prefix(project.layout().root())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let mut request = request();
        request.draft.mode = GenerationMode::Edit;
        request.draft.references.push(ReferenceAssetDto {
            id: "reference".to_owned(),
            name: "reference.png".to_owned(),
            url: relative,
            mime_type: "image/png".to_owned(),
            role: ReferenceRole::Source,
            source_type: Some(ReferenceSourceType::Local),
            file_id: None,
        });

        let prepared = prepare_generation_request(&project, &request)
            .await
            .unwrap();

        assert_eq!(
            prepared.provider_request.operation,
            provider::Operation::Edit
        );
        assert_eq!(prepared.provider_request.inputs.len(), 1);
        assert!(matches!(
            &prepared.provider_request.inputs[0],
            provider::InputAsset::LocalFile { path, .. }
                if path.starts_with(project.layout().root())
        ));
    }

    #[test]
    fn maps_gemini_to_single_output_provider_requests() {
        let request = request();
        let output = provider_output(&request, provider::ProviderKind::Gemini);
        assert_eq!(output.count, 1);
        assert_eq!(output.resolution.as_deref(), Some("1K"));
    }

    #[test]
    fn maps_embedded_capability_catalog() {
        let resolved = capabilities::resolve_model(
            provider::ProviderKind::Gemini,
            "gemini-3.1-flash-image",
            None,
        )
        .unwrap()
        .unwrap();
        let capability = map_resolved_capability(
            provider::ProviderKind::Gemini,
            "gemini-3.1-flash-image",
            &resolved,
        );
        assert!(capability.supports_thinking);
        assert_eq!(capability.max_reference_images, 14);
    }

    #[test]
    fn default_openai_optional_values_stay_on_images_api() {
        let mut request = request();
        request.provider.kind = FrontendProviderKind::Openai;
        request.draft.model = "gpt-image-1".to_owned();
        request.draft.previous_response_id = Some(String::new());
        request.draft.image_generation_action = Some("auto".to_owned());
        request.draft.style = Some("vivid".to_owned());

        let options = provider_options(&request, provider::ProviderKind::OpenAi, &Map::new())
            .unwrap()
            .openai
            .unwrap();

        assert_eq!(options.api_surface, provider::OpenAiApiSurface::Images);
        assert!(options.previous_response_id.is_none());
        assert!(options.image_generation_action.is_none());
        assert!(options.style.is_none());
    }

    #[test]
    fn empty_gemini_interaction_ids_are_omitted() {
        let mut request = request();
        request.draft.previous_interaction_id = Some("  ".to_owned());
        request.draft.resume_interaction_id = Some(String::new());
        request.draft.last_event_id = Some(" ".to_owned());

        let options = provider_options(&request, provider::ProviderKind::Gemini, &Map::new())
            .unwrap()
            .gemini
            .unwrap();

        assert!(options.previous_interaction_id.is_none());
        assert!(options.resume_interaction_id.is_none());
        assert!(options.last_event_id.is_none());
    }

    #[test]
    fn gemini_surface_selection_is_explicit_and_stream_independent() {
        let mut request = request();
        let default_options =
            provider_options(&request, provider::ProviderKind::Gemini, &Map::new())
                .unwrap()
                .gemini
                .unwrap();
        assert_eq!(
            default_options.api_surface,
            provider::GeminiApiSurface::GenerateContent
        );

        request.draft.stream = true;
        request.draft.use_interactions_api = Some(true);
        let interactions = provider_options(&request, provider::ProviderKind::Gemini, &Map::new())
            .unwrap()
            .gemini
            .unwrap();
        assert_eq!(
            interactions.api_surface,
            provider::GeminiApiSurface::Interactions
        );
        assert!(interactions.stream);

        request.draft.batch = true;
        let batch = provider_options(&request, provider::ProviderKind::Gemini, &Map::new())
            .unwrap()
            .gemini
            .unwrap();
        assert_eq!(
            batch.api_surface,
            provider::GeminiApiSurface::GenerateContent
        );
    }

    #[tokio::test]
    async fn prepared_request_and_history_snapshot_share_resolved_override() {
        let directory = tempfile::tempdir().unwrap();
        let project = ProjectStore::create(directory.path().join("project"), "Project")
            .await
            .unwrap();
        let mut request = request();
        request.context_ids = vec!["context-a".to_owned()];
        request.preset_id = Some("preset-a".to_owned());
        request.draft.negative_prompt = "project negative\nmanual negative".to_owned();
        request.manual_negative_prompt = Some("manual negative".to_owned());
        request.draft.custom_json = serde_json::json!({
            "api_key": "must-not-persist",
            "payload": format!("data:image/png;base64,{}", "a".repeat(500))
        })
        .to_string();
        request.provider.capability_overrides_json = Some(
            serde_json::json!({
                "gemini-3.1-flash-image": {
                    "output_count": { "min": 1, "max": 4 },
                    "features": { "thinking": false }
                }
            })
            .to_string(),
        );

        let prepared = prepare_generation_request(&project, &request)
            .await
            .unwrap();
        let resolved = prepared
            .provider_request
            .resolved_capability
            .as_deref()
            .unwrap();
        assert_eq!(resolved.id, "gemini-3.1-flash-image");
        assert_eq!(resolved.output_count.as_ref().unwrap().max, 4);
        assert!(!resolved.features.thinking);
        assert_eq!(prepared.capability_snapshot.max_outputs, 4);
        assert!(!prepared.capability_snapshot.supports_thinking);
        assert_eq!(prepared.domain_request.context_ids, ["context-a"]);
        assert_eq!(
            prepared.domain_request.preset_id.as_deref(),
            Some("preset-a")
        );
        let frontend_draft = &prepared.domain_request.metadata["frontendDraft"];
        assert_eq!(frontend_draft["prompt"], "city");
        assert_eq!(frontend_draft["negativePrompt"], "manual negative");
        assert!(
            prepared
                .provider_request
                .prompt
                .contains("project negative")
        );
        assert!(frontend_draft.get("references").is_none());
        assert!(frontend_draft.get("maskDataUrl").is_none());
        assert_eq!(
            prepared.domain_request.metadata["composedPromptBase"],
            "city"
        );
        assert_eq!(prepared.domain_request.parameters["api_key"], "[REDACTED]");
        assert!(
            prepared.domain_request.parameters["payload"]
                .as_str()
                .unwrap()
                .starts_with("[OMITTED")
        );
        assert!(
            serde_json::to_value(&prepared.provider_request)
                .unwrap()
                .get("resolved_capability")
                .is_none()
        );
    }

    #[test]
    fn validates_mask_alpha_and_matching_dimensions() {
        let directory = tempfile::tempdir().unwrap();
        let source_path = directory.path().join("source.png");
        let source = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            8,
            8,
            image::Rgba([1, 2, 3, 255]),
        ));
        source.save(&source_path).unwrap();
        let mut mask_bytes = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            8,
            8,
            image::Rgba([0, 0, 0, 0]),
        ))
        .write_to(&mut mask_bytes, image::ImageFormat::Png)
        .unwrap();
        let mask = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(mask_bytes.into_inner())
        );
        let inputs = vec![provider::InputAsset::LocalFile {
            path: source_path,
            mime_type: "image/png".to_owned(),
            label: None,
        }];

        assert!(validate_mask(&mask, &inputs).is_ok());
    }

    #[test]
    fn rejects_mask_without_alpha() {
        let directory = tempfile::tempdir().unwrap();
        let source_path = directory.path().join("source.png");
        image::DynamicImage::ImageRgb8(image::RgbImage::new(8, 8))
            .save(&source_path)
            .unwrap();
        let bytes = std::fs::read(&source_path).unwrap();
        let mask = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        );
        let inputs = vec![provider::InputAsset::LocalFile {
            path: source_path,
            mime_type: "image/png".to_owned(),
            label: None,
        }];

        assert!(validate_mask(&mask, &inputs).is_err());
    }
}
