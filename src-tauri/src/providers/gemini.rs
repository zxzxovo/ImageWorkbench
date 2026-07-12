use std::time::Instant;

use base64::Engine;
use reqwest::Method;
use reqwest::header::CONTENT_TYPE;
use serde_json::{Map, Value, json};

use super::capabilities::find_model;
use super::error::{ProviderError, ProviderErrorKind};
use super::http::{HttpTransport, decode_base64_asset, encode_path_segment, parse_error_response};
use super::types::{
    AssetSource, BatchSubmission, ConnectionTest, DiscoveredModel, DownloadedAsset, EventHandler,
    ExecutionMode, GeminiApiSurface, GeminiOptions, GenerationRequest, GenerationResponse,
    InputAsset, JobPollResult, Operation, OutputPart, ProviderConfig, ProviderCredentials,
    ProviderKind, RemoteJob, RemoteJobKind, RemoteJobStatus, RunEvent, UsageRecord,
};
use super::{ProviderAdapter, ProviderFuture};

const MAX_INLINE_BYTES: u64 = 20 * 1024 * 1024;
const INTERACTIONS_API_REVISION: &str = "2026-05-20";

pub struct GeminiAdapter {
    transport: HttpTransport,
}

impl GeminiAdapter {
    pub fn new(
        config: ProviderConfig,
        credentials: ProviderCredentials,
    ) -> Result<Self, ProviderError> {
        Ok(Self {
            transport: HttpTransport::new(config, credentials)?,
        })
    }

    fn options(&self, request: &GenerationRequest) -> GeminiOptions {
        request.options.gemini.clone().unwrap_or_default()
    }

    async fn execute_interaction(
        &self,
        request: &GenerationRequest,
        options: &GeminiOptions,
    ) -> Result<GenerationResponse, ProviderError> {
        let body = build_interactions_body(request, options)?;
        let response = self
            .transport
            .send_json(
                self.transport
                    .request(Method::POST, "/interactions")?
                    .header("Api-Revision", INTERACTIONS_API_REVISION)
                    .json(&body),
            )
            .await?;
        normalize_interaction_response(response.value, response.request_id)
    }

    async fn execute_generate_content(
        &self,
        request: &GenerationRequest,
        options: &GeminiOptions,
    ) -> Result<GenerationResponse, ProviderError> {
        let body = build_generate_content_body(request, options)?;
        let response = self
            .transport
            .send_json(
                self.transport
                    .request(
                        Method::POST,
                        &format!("/models/{}:generateContent", request.model),
                    )?
                    .json(&body),
            )
            .await?;
        normalize_generate_content_response(response.value, response.request_id)
    }

    fn download_url_for_file(&self, id: &str) -> Result<String, ProviderError> {
        let base = reqwest::Url::parse(&self.transport.config().base_url)
            .map_err(|error| ProviderError::validation(error.to_string()))?;
        let host = base
            .host_str()
            .ok_or_else(|| ProviderError::validation("Gemini base URL has no host"))?;
        let scheme = base.scheme();
        let version = self
            .transport
            .config()
            .api_version
            .as_deref()
            .unwrap_or("v1beta");
        Ok(format!(
            "{scheme}://{host}/download/{version}/{}:download?alt=media",
            id.trim_start_matches('/')
        ))
    }

    async fn download_batch_outputs(
        &self,
        batch: &Value,
    ) -> Result<Vec<GenerationResponse>, ProviderError> {
        let Some(file_name) = batch
            .pointer("/dest/fileName")
            .or_else(|| batch.pointer("/dest/file_name"))
            .and_then(Value::as_str)
        else {
            return Ok(Vec::new());
        };
        let url = self.download_url_for_file(file_name)?;
        let response = self
            .transport
            .request_url(Method::GET, &url)?
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(parse_error_response(response).await);
        }
        let bytes = response.bytes().await?;
        let text = String::from_utf8(bytes.to_vec()).map_err(|error| {
            ProviderError::parse(
                format!("Gemini batch output is not UTF-8 JSONL: {error}"),
                None,
            )
        })?;
        let mut outputs = Vec::new();
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let item: Value = serde_json::from_str(line)?;
            if let Some(response) = item.get("response") {
                outputs.push(normalize_generate_content_response(
                    response.clone(),
                    item.get("key")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                )?);
            }
        }
        Ok(outputs)
    }

    fn interaction_request(
        &self,
        method: Method,
        path: &str,
    ) -> Result<reqwest::RequestBuilder, ProviderError> {
        Ok(self
            .transport
            .request(method, path)?
            .header("Api-Revision", INTERACTIONS_API_REVISION))
    }
}

impl ProviderAdapter for GeminiAdapter {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Gemini
    }

    fn config(&self) -> &ProviderConfig {
        self.transport.config()
    }

    fn test_connection(&self) -> ProviderFuture<'_, ConnectionTest> {
        Box::pin(async move {
            let started = Instant::now();
            let models = self.list_models().await?;
            Ok(ConnectionTest {
                provider: ProviderKind::Gemini,
                latency_ms: started.elapsed().as_millis(),
                model_count: models.len(),
            })
        })
    }

    fn list_models(&self) -> ProviderFuture<'_, Vec<DiscoveredModel>> {
        Box::pin(async move {
            let path = self
                .transport
                .config()
                .models_path
                .as_deref()
                .unwrap_or("/models?pageSize=1000");
            let response = self
                .transport
                .send_json(self.transport.request(Method::GET, path)?)
                .await?;
            let models = response
                .value
                .get("models")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    ProviderError::parse(
                        "Gemini model response has no models array",
                        Some(response.value.clone()),
                    )
                })?;
            Ok(models
                .iter()
                .filter_map(|model| {
                    let name = model.get("name")?.as_str()?;
                    let id = name.strip_prefix("models/").unwrap_or(name).to_owned();
                    let supported_actions = model
                        .get("supportedGenerationMethods")
                        .or_else(|| model.get("supported_generation_methods"))
                        .and_then(Value::as_array)
                        .map(|methods| {
                            methods
                                .iter()
                                .filter_map(Value::as_str)
                                .map(ToOwned::to_owned)
                                .collect()
                        })
                        .unwrap_or_default();
                    (id.contains("image") || id.contains("nano-banana")).then(|| DiscoveredModel {
                        id,
                        display_name: model
                            .get("displayName")
                            .or_else(|| model.get("display_name"))
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned),
                        owned_by: Some("google".into()),
                        supported_actions,
                        raw: model.clone(),
                    })
                })
                .collect())
        })
    }

    fn validate(&self, request: &GenerationRequest) -> Result<(), ProviderError> {
        validate_gemini_request(request)
    }

    fn execute<'a>(
        &'a self,
        request: &'a GenerationRequest,
    ) -> ProviderFuture<'a, GenerationResponse> {
        Box::pin(async move {
            self.validate(request)?;
            if request.execution == ExecutionMode::ProviderBatch {
                return Err(ProviderError::unsupported(
                    "submit Gemini batches through submit_batch",
                ));
            }
            let options = self.options(request);
            if options.stream {
                return Err(ProviderError::unsupported(
                    "streaming requests must be invoked through execute_stream",
                ));
            }
            match options.api_surface {
                GeminiApiSurface::Interactions => self.execute_interaction(request, &options).await,
                GeminiApiSurface::GenerateContent => {
                    self.execute_generate_content(request, &options).await
                }
            }
        })
    }

    fn execute_stream<'a>(
        &'a self,
        request: &'a GenerationRequest,
        events: EventHandler,
    ) -> ProviderFuture<'a, GenerationResponse> {
        Box::pin(async move {
            self.validate(request)?;
            let options = self.options(request);
            match options.api_surface {
                GeminiApiSurface::GenerateContent => {
                    let body = build_generate_content_body(request, &options)?;
                    let response = self
                        .transport
                        .request(
                            Method::POST,
                            &format!("/models/{}:streamGenerateContent?alt=sse", request.model),
                        )?
                        .json(&body)
                        .send()
                        .await?;
                    if !response.status().is_success() {
                        return Err(parse_error_response(response).await);
                    }
                    consume_generate_content_sse(request, response, events).await
                }
                GeminiApiSurface::Interactions => {
                    let response =
                        if let Some(interaction_id) = options.resume_interaction_id.as_deref() {
                            let mut path = format!(
                                "/interactions/{}?stream=true",
                                interaction_id.trim_start_matches("interactions/")
                            );
                            if let Some(last_event_id) = options.last_event_id.as_deref() {
                                let encoded: String = reqwest::Url::parse_with_params(
                                    "https://resume.invalid/",
                                    &[("last_event_id", last_event_id)],
                                )
                                .map_err(|error| ProviderError::validation(error.to_string()))?
                                .query()
                                .unwrap_or_default()
                                .to_owned();
                                path.push('&');
                                path.push_str(&encoded);
                            }
                            self.interaction_request(Method::GET, &path)?.send().await?
                        } else {
                            let mut body = build_interactions_body(request, &options)?;
                            body["stream"] = Value::Bool(true);
                            self.interaction_request(Method::POST, "/interactions")?
                                .json(&body)
                                .send()
                                .await?
                        };
                    if !response.status().is_success() {
                        return Err(parse_error_response(response).await);
                    }
                    consume_interactions_sse(request, response, events).await
                }
            }
        })
    }

    fn submit_batch<'a>(
        &'a self,
        submission: &'a BatchSubmission,
    ) -> ProviderFuture<'a, RemoteJob> {
        Box::pin(async move {
            let model = submission
                .model
                .as_deref()
                .ok_or_else(|| ProviderError::validation("Gemini batch model is required"))?;
            let input_config = if let Some(file_name) = &submission.input_file_id {
                json!({ "file_name": file_name })
            } else {
                if submission.requests.is_empty() {
                    return Err(ProviderError::validation(
                        "Gemini batch requests cannot be empty",
                    ));
                }
                let requests = submission
                    .requests
                    .iter()
                    .map(|item| {
                        json!({
                            "request": item.body,
                            "metadata": { "key": item.key }
                        })
                    })
                    .collect::<Vec<_>>();
                json!({ "requests": { "requests": requests } })
            };
            let body = json!({
                "batch": {
                    "display_name": submission.name,
                    "input_config": input_config
                }
            });
            let response = self
                .transport
                .send_json(
                    self.transport
                        .request(
                            Method::POST,
                            &format!("/models/{model}:batchGenerateContent"),
                        )?
                        .json(&body),
                )
                .await?;
            parse_gemini_job(response.value, RemoteJobKind::Batch)
        })
    }

    fn poll_job<'a>(&'a self, job: &'a RemoteJob) -> ProviderFuture<'a, JobPollResult> {
        Box::pin(async move {
            let path = match job.kind {
                RemoteJobKind::Batch => format!("/{}", job.id.trim_start_matches('/')),
                RemoteJobKind::Background => {
                    if job.id.starts_with("interactions/") {
                        format!("/{}", job.id)
                    } else {
                        format!("/interactions/{}", job.id)
                    }
                }
            };
            let response = self
                .transport
                .send_json(if job.kind == RemoteJobKind::Background {
                    self.interaction_request(Method::GET, &path)?
                } else {
                    self.transport.request(Method::GET, &path)?
                })
                .await?;
            if job.kind == RemoteJobKind::Background {
                let generation =
                    normalize_interaction_response(response.value.clone(), response.request_id)?;
                let remote_job = generation.remote_job.clone().unwrap_or(RemoteJob {
                    id: job.id.clone(),
                    kind: job.kind,
                    status: RemoteJobStatus::Succeeded,
                    provider: ProviderKind::Gemini,
                    model: generation.model.clone(),
                    raw: response.value,
                });
                return Ok(JobPollResult {
                    job: remote_job,
                    outputs: vec![generation],
                    next_page_token: None,
                });
            }
            let remote_job = parse_gemini_job(response.value.clone(), RemoteJobKind::Batch)?;
            let mut outputs = Vec::new();
            for item in response
                .value
                .pointer("/dest/inlinedResponses")
                .or_else(|| response.value.pointer("/dest/inlined_responses"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(value) = item.get("response") {
                    outputs.push(normalize_generate_content_response(value.clone(), None)?);
                }
            }
            if outputs.is_empty() && remote_job.status == RemoteJobStatus::Succeeded {
                outputs = self.download_batch_outputs(&response.value).await?;
            }
            Ok(JobPollResult {
                job: remote_job,
                outputs,
                next_page_token: None,
            })
        })
    }

    fn cancel_job<'a>(&'a self, job: &'a RemoteJob) -> ProviderFuture<'a, RemoteJob> {
        Box::pin(async move {
            let path = match job.kind {
                RemoteJobKind::Batch => format!("/{}:cancel", job.id.trim_start_matches('/')),
                RemoteJobKind::Background => {
                    if job.id.starts_with("interactions/") {
                        format!("/{}/cancel", job.id)
                    } else {
                        format!("/interactions/{}/cancel", job.id)
                    }
                }
            };
            let response = self
                .transport
                .send_json(if job.kind == RemoteJobKind::Background {
                    self.interaction_request(Method::POST, &path)?
                } else {
                    self.transport.request(Method::POST, &path)?
                })
                .await?;
            if response.value.is_null() {
                let mut cancelled = job.clone();
                cancelled.status = RemoteJobStatus::Cancelled;
                return Ok(cancelled);
            }
            parse_gemini_job(response.value, job.kind)
        })
    }

    fn delete_file<'a>(&'a self, file_id: &'a str) -> ProviderFuture<'a, ()> {
        Box::pin(async move {
            let name = file_id
                .trim()
                .strip_prefix("files/")
                .unwrap_or(file_id.trim());
            let name = encode_path_segment(name)?;
            self.transport
                .send_json(
                    self.transport
                        .request(Method::DELETE, &format!("/files/{name}"))?,
                )
                .await?;
            Ok(())
        })
    }

    fn download_asset<'a>(
        &'a self,
        source: &'a AssetSource,
    ) -> ProviderFuture<'a, DownloadedAsset> {
        Box::pin(async move {
            if let AssetSource::FileId { id } = source {
                let url = self.download_url_for_file(id)?;
                let response = self
                    .transport
                    .request_url(Method::GET, &url)?
                    .send()
                    .await?;
                if !response.status().is_success() {
                    return Err(parse_error_response(response).await);
                }
                let mime_type = response
                    .headers()
                    .get(CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .map(ToOwned::to_owned);
                return Ok(DownloadedAsset {
                    bytes: response.bytes().await?.to_vec(),
                    mime_type,
                    filename: id.split('/').next_back().map(ToOwned::to_owned),
                });
            }
            self.transport.download(source).await
        })
    }
}

pub(crate) fn validate_gemini_request(request: &GenerationRequest) -> Result<(), ProviderError> {
    if request.model.trim().is_empty() {
        return Err(ProviderError::validation("model is required"));
    }
    if request.prompt.trim().is_empty() {
        return Err(ProviderError::validation("prompt is required"));
    }
    let catalog_capability = if request.resolved_capability.is_some() {
        None
    } else {
        find_model(ProviderKind::Gemini, &request.model)?
    };
    let capability = request
        .resolved_capability
        .as_deref()
        .or(catalog_capability)
        .ok_or_else(|| {
            ProviderError::validation(format!(
                "unknown Gemini image model {}; refresh model capabilities",
                request.model
            ))
        })?;
    if !capability.operations.contains(&request.operation) {
        return Err(ProviderError::validation(format!(
            "{} does not support {:?}",
            request.model, request.operation
        )));
    }
    if let Some(limit) = &capability.output_count {
        let count = u32::from(request.output.count);
        if count < limit.min || count > limit.max {
            return Err(ProviderError::validation(format!(
                "Gemini output count must be between {} and {}; split larger desired counts into separate tasks",
                limit.min, limit.max
            )));
        }
    }
    if let Some(max_images) = capability.max_input_images {
        let image_count = request
            .inputs
            .iter()
            .filter(|input| !input.is_video())
            .count();
        if image_count > usize::from(max_images) {
            return Err(ProviderError::validation(format!(
                "{} accepts at most {max_images} reference images",
                request.model
            )));
        }
    }
    match request.operation {
        Operation::Generate if !request.inputs.is_empty() => {
            return Err(ProviderError::validation(
                "use edit or video-reference mode when Gemini inputs are present",
            ));
        }
        Operation::Edit if request.inputs.is_empty() => {
            return Err(ProviderError::validation(
                "Gemini editing requires an input image",
            ));
        }
        Operation::VideoReferenceToImage => {
            if !capability.features.video_input || !request.inputs.iter().any(InputAsset::is_video)
            {
                return Err(ProviderError::validation(
                    "video-to-image requires Gemini 3.1 Flash Image and a video input",
                ));
            }
        }
        _ => {
            if request.inputs.iter().any(InputAsset::is_video) {
                return Err(ProviderError::validation(
                    "video inputs require video-reference-to-image mode",
                ));
            }
        }
    }
    if request.mask.is_some() {
        return Err(ProviderError::unsupported(
            "Gemini image editing does not expose a mask parameter",
        ));
    }
    if request
        .inputs
        .iter()
        .any(|input| matches!(input, InputAsset::FileId { .. }))
        && !capability.features.file_inputs
    {
        return Err(ProviderError::validation(format!(
            "{} does not support provider file inputs",
            capability.id
        )));
    }
    if let Some(aspect_ratio) = request.output.aspect_ratio.as_deref() {
        if !capability
            .aspect_ratios
            .iter()
            .any(|allowed| allowed == aspect_ratio)
        {
            return Err(ProviderError::validation(format!(
                "aspect ratio {aspect_ratio} is not supported by {}",
                request.model
            )));
        }
    }
    if let Some(resolution) = request.output.resolution.as_deref() {
        if !capability
            .resolutions
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(resolution))
        {
            return Err(ProviderError::validation(format!(
                "resolution {resolution} is not supported by {}",
                request.model
            )));
        }
    }
    if request.output.size.is_some()
        || request.output.quality.is_some()
        || request.output.response_format.is_some()
        || request.output.background.is_some()
        || request.output.compression.is_some()
    {
        return Err(ProviderError::validation(
            "Gemini uses aspect_ratio, resolution and response modalities instead of size, quality, response format, background or compression",
        ));
    }
    if request.output.format.as_deref().is_some_and(|format| {
        !format.eq_ignore_ascii_case("image/png") && !format.eq_ignore_ascii_case("png")
    }) {
        return Err(ProviderError::validation(
            "Gemini native image output is PNG",
        ));
    }
    let options = request.options.gemini.clone().unwrap_or_default();
    if options
        .temperature
        .is_some_and(|value| !value.is_finite() || !(0.0..=2.0).contains(&value))
    {
        return Err(ProviderError::validation(
            "Gemini temperature must be between 0 and 2",
        ));
    }
    if options
        .top_p
        .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
    {
        return Err(ProviderError::validation(
            "Gemini top_p must be between 0 and 1",
        ));
    }
    if options.stream && !capability.features.streaming {
        return Err(ProviderError::validation(format!(
            "{} does not support streaming",
            capability.id
        )));
    }
    if request.execution == ExecutionMode::Background && !capability.features.background {
        return Err(ProviderError::validation(format!(
            "{} does not support background execution",
            capability.id
        )));
    }
    if request.execution == ExecutionMode::ProviderBatch && !capability.features.batch {
        return Err(ProviderError::validation(format!(
            "{} does not support provider batches",
            capability.id
        )));
    }
    if !options
        .response_modalities
        .iter()
        .any(|modality| modality.eq_ignore_ascii_case("IMAGE"))
    {
        return Err(ProviderError::validation(
            "Gemini image generation response modalities must include IMAGE",
        ));
    }
    if options.response_modalities.iter().any(|modality| {
        !modality.eq_ignore_ascii_case("IMAGE") && !modality.eq_ignore_ascii_case("TEXT")
    }) {
        return Err(ProviderError::validation(
            "Gemini image response modalities may only contain IMAGE and TEXT",
        ));
    }
    if options
        .response_modalities
        .iter()
        .any(|modality| modality.eq_ignore_ascii_case("TEXT"))
        && !capability.features.interleaved_text
    {
        return Err(ProviderError::validation(
            "this Gemini model does not support interleaved text output",
        ));
    }
    if (options.thinking_level.is_some() || options.include_thoughts)
        && !capability.features.thinking
    {
        return Err(ProviderError::validation(
            "this Gemini model does not expose thinking controls",
        ));
    }
    if let Some(level) = options.thinking_level.as_deref() {
        if !["minimal", "low", "medium", "high"].contains(&level) {
            return Err(ProviderError::validation(
                "Gemini thinking level must be minimal, low, medium or high",
            ));
        }
    }
    if options.google_search && !capability.features.google_search {
        return Err(ProviderError::validation(
            "this Gemini model does not support Google Search",
        ));
    }
    if options.image_search && !capability.features.image_search {
        return Err(ProviderError::validation(
            "this Gemini model does not support Image Search",
        ));
    }
    if options.image_search && options.api_surface != GeminiApiSurface::Interactions {
        return Err(ProviderError::validation(
            "Image Search requires the Gemini Interactions API surface",
        ));
    }
    if options.resume_interaction_id.is_some()
        && options.api_surface != GeminiApiSurface::Interactions
    {
        return Err(ProviderError::validation(
            "stream resumption requires the Gemini Interactions API surface",
        ));
    }
    if options.last_event_id.is_some() && options.resume_interaction_id.is_none() {
        return Err(ProviderError::validation(
            "last_event_id requires resume_interaction_id",
        ));
    }
    if request.execution == ExecutionMode::Background
        && options.api_surface != GeminiApiSurface::Interactions
    {
        return Err(ProviderError::validation(
            "Gemini background execution requires the Interactions API surface",
        ));
    }
    if inline_input_size(&request.inputs)? > MAX_INLINE_BYTES {
        return Err(ProviderError::validation(
            "inline Gemini inputs exceed 20 MB; upload them through the Files API and use File IDs",
        ));
    }
    Ok(())
}

pub(crate) fn build_interactions_body(
    request: &GenerationRequest,
    options: &GeminiOptions,
) -> Result<Value, ProviderError> {
    let mut input = Vec::new();
    for asset in &request.inputs {
        input.push(interaction_input(asset)?);
    }
    input.push(json!({ "type": "text", "text": request.prompt }));

    let image_format = gemini_image_format(request);
    let response_format = if options.response_modalities.len() == 1 {
        image_format
    } else {
        let mut formats = Vec::new();
        if options
            .response_modalities
            .iter()
            .any(|modality| modality.eq_ignore_ascii_case("TEXT"))
        {
            formats.push(json!({ "type": "text" }));
        }
        formats.push(image_format);
        Value::Array(formats)
    };
    let mut body = json!({
        "model": request.model,
        "input": input,
        "response_format": response_format,
    });
    if let Some(previous) = options.previous_interaction_id.as_deref() {
        body["previous_interaction_id"] = Value::String(previous.into());
    }
    let mut generation_config = Map::new();
    if let Some(thinking_level) = options.thinking_level.as_deref() {
        generation_config.insert(
            "thinking_level".into(),
            Value::String(thinking_level.into()),
        );
    }
    if options.include_thoughts {
        generation_config.insert("thinking_summaries".into(), Value::String("auto".into()));
    }
    if let Some(temperature) = options.temperature {
        generation_config.insert("temperature".into(), Value::from(temperature));
    }
    if let Some(top_p) = options.top_p {
        generation_config.insert("top_p".into(), Value::from(top_p));
    }
    if !generation_config.is_empty() {
        body["generation_config"] = Value::Object(generation_config);
    }
    let mut tools = Vec::new();
    if options.google_search {
        tools.push(json!({ "type": "google_search" }));
    }
    if options.image_search {
        tools.push(json!({ "type": "image_search" }));
    }
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
    }
    if request.execution == ExecutionMode::Background {
        body["background"] = Value::Bool(true);
    }
    if let Some(object) = body.as_object_mut() {
        merge_extra(object, &options.extra);
    }
    Ok(body)
}

pub(crate) fn build_generate_content_body(
    request: &GenerationRequest,
    options: &GeminiOptions,
) -> Result<Value, ProviderError> {
    let mut parts = vec![json!({ "text": request.prompt })];
    for asset in &request.inputs {
        parts.push(generate_content_input(asset)?);
    }
    let mut generation_config = json!({
        "responseModalities": options
            .response_modalities
            .iter()
            .map(|modality| modality.to_ascii_uppercase())
            .collect::<Vec<_>>(),
        "imageConfig": {}
    });
    if let Some(aspect_ratio) = &request.output.aspect_ratio {
        generation_config["imageConfig"]["aspectRatio"] = Value::String(aspect_ratio.clone());
    }
    if let Some(resolution) = &request.output.resolution {
        generation_config["imageConfig"]["imageSize"] =
            Value::String(resolution.to_ascii_uppercase());
    }
    if let Some(temperature) = options.temperature {
        generation_config["temperature"] = Value::from(temperature);
    }
    if let Some(top_p) = options.top_p {
        generation_config["topP"] = Value::from(top_p);
    }
    let mut body = json!({
        "contents": [{ "role": "user", "parts": parts }],
        "generationConfig": generation_config,
    });
    if let Some(level) = options.thinking_level.as_deref() {
        body["generationConfig"]["thinkingConfig"] = json!({
            "thinkingLevel": level,
            "includeThoughts": options.include_thoughts
        });
    } else if options.include_thoughts {
        body["generationConfig"]["thinkingConfig"] = json!({ "includeThoughts": true });
    }
    if options.google_search {
        body["tools"] = json!([{ "googleSearch": {} }]);
    }
    if let Some(object) = body.as_object_mut() {
        merge_extra(object, &options.extra);
    }
    Ok(body)
}

fn interaction_input(asset: &InputAsset) -> Result<Value, ProviderError> {
    match asset {
        InputAsset::Url { url, mime_type, .. } => Ok(json!({
            "type": if asset.is_video() { "video" } else { "image" },
            "uri": url,
            "mime_type": mime_type,
        })),
        InputAsset::FileId { id, mime_type, .. } => Ok(json!({
            "type": if asset.is_video() { "video" } else { "image" },
            "uri": id,
            "mime_type": mime_type,
        })),
        InputAsset::Base64 {
            data, mime_type, ..
        } => Ok(json!({
            "type": if asset.is_video() { "video" } else { "image" },
            "data": plain_base64(data)?,
            "mime_type": mime_type,
        })),
        InputAsset::LocalFile {
            path, mime_type, ..
        } => Ok(json!({
            "type": if asset.is_video() { "video" } else { "image" },
            "data": base64::engine::general_purpose::STANDARD.encode(
                std::fs::read(path).map_err(ProviderError::io)?
            ),
            "mime_type": mime_type,
        })),
    }
}

fn generate_content_input(asset: &InputAsset) -> Result<Value, ProviderError> {
    match asset {
        InputAsset::Url { url, mime_type, .. } => Ok(json!({
            "fileData": { "fileUri": url, "mimeType": mime_type }
        })),
        InputAsset::FileId { id, mime_type, .. } => Ok(json!({
            "fileData": { "fileUri": id, "mimeType": mime_type }
        })),
        InputAsset::Base64 {
            data, mime_type, ..
        } => Ok(json!({
            "inlineData": { "data": plain_base64(data)?, "mimeType": mime_type }
        })),
        InputAsset::LocalFile {
            path, mime_type, ..
        } => Ok(json!({
            "inlineData": {
                "data": base64::engine::general_purpose::STANDARD.encode(
                    std::fs::read(path).map_err(ProviderError::io)?
                ),
                "mimeType": mime_type
            }
        })),
    }
}

fn gemini_image_format(request: &GenerationRequest) -> Value {
    let mut format = json!({ "type": "image" });
    if let Some(aspect_ratio) = &request.output.aspect_ratio {
        format["aspect_ratio"] = Value::String(aspect_ratio.clone());
    }
    if let Some(resolution) = &request.output.resolution {
        format["image_size"] = Value::String(resolution.to_ascii_uppercase());
    }
    format
}

pub(crate) fn normalize_interaction_response(
    value: Value,
    request_id: Option<String>,
) -> Result<GenerationResponse, ProviderError> {
    let mut outputs = Vec::new();
    for step in value
        .get("steps")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match step.get("type").and_then(Value::as_str) {
            Some("model_output") => {
                for content in step
                    .get("content")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    parse_interaction_content(content, &mut outputs);
                }
            }
            Some("thought") => {
                let text = step
                    .get("summary")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|item| item.get("text").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join("\n");
                if !text.is_empty() {
                    outputs.push(OutputPart::Thought {
                        text,
                        signature: step
                            .get("signature")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned),
                    });
                }
            }
            Some("google_search_result") | Some("image_search_result") => {
                if let Some(html) = step.get("search_suggestions").and_then(Value::as_str) {
                    outputs.push(OutputPart::SearchSuggestions {
                        html: html.into(),
                        signature: step
                            .get("signature")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned),
                    });
                }
                for result in step
                    .get("result")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    outputs.push(OutputPart::Citation {
                        title: result
                            .get("title")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned),
                        url: result
                            .get("url")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned),
                        snippet: result
                            .get("snippet")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned),
                        raw: result.clone(),
                    });
                }
            }
            _ => {}
        }
    }
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .map(map_gemini_status)
        .unwrap_or(RemoteJobStatus::Unknown);
    let remote_job = if matches!(status, RemoteJobStatus::Queued | RemoteJobStatus::Running) {
        value
            .get("id")
            .or_else(|| value.get("name"))
            .and_then(Value::as_str)
            .map(|id| RemoteJob {
                id: id.into(),
                kind: RemoteJobKind::Background,
                status,
                provider: ProviderKind::Gemini,
                model: value
                    .get("model")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                raw: value.clone(),
            })
    } else {
        None
    };
    Ok(GenerationResponse {
        provider: ProviderKind::Gemini,
        model: value
            .get("model")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        created_at: value
            .get("created_at")
            .or_else(|| value.get("create_time"))
            .and_then(Value::as_i64),
        outputs,
        usage: value
            .get("usage")
            .or_else(|| value.get("usage_metadata"))
            .map(parse_gemini_usage),
        remote_job,
        request_id,
        raw: value,
    })
}

pub(crate) fn normalize_generate_content_response(
    value: Value,
    request_id: Option<String>,
) -> Result<GenerationResponse, ProviderError> {
    let mut outputs = Vec::new();
    for candidate in value
        .get("candidates")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        for part in candidate
            .pointer("/content/parts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(inline) = part.get("inlineData").or_else(|| part.get("inline_data")) {
                if let Some(data) = inline.get("data").and_then(Value::as_str) {
                    outputs.push(OutputPart::Image {
                        source: AssetSource::Base64 { data: data.into() },
                        mime_type: inline
                            .get("mimeType")
                            .or_else(|| inline.get("mime_type"))
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned),
                        revised_prompt: None,
                        remote_file: None,
                    });
                }
            } else if let Some(file) = part.get("fileData").or_else(|| part.get("file_data")) {
                if let Some(uri) = file
                    .get("fileUri")
                    .or_else(|| file.get("file_uri"))
                    .and_then(Value::as_str)
                {
                    outputs.push(OutputPart::Image {
                        source: AssetSource::Url { url: uri.into() },
                        mime_type: file
                            .get("mimeType")
                            .or_else(|| file.get("mime_type"))
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned),
                        revised_prompt: None,
                        remote_file: None,
                    });
                }
            } else if let Some(text) = part.get("text").and_then(Value::as_str) {
                if part
                    .get("thought")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    outputs.push(OutputPart::Thought {
                        text: text.into(),
                        signature: part
                            .get("thoughtSignature")
                            .or_else(|| part.get("thought_signature"))
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned),
                    });
                } else {
                    outputs.push(OutputPart::Text {
                        text: text.into(),
                        annotations: Vec::new(),
                    });
                }
            }
        }
        parse_grounding(candidate.get("groundingMetadata"), &mut outputs);
    }
    Ok(GenerationResponse {
        provider: ProviderKind::Gemini,
        model: value
            .get("modelVersion")
            .or_else(|| value.get("model_version"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        created_at: None,
        outputs,
        usage: value
            .get("usageMetadata")
            .or_else(|| value.get("usage_metadata"))
            .map(parse_gemini_usage),
        remote_job: None,
        request_id,
        raw: value,
    })
}

fn parse_interaction_content(content: &Value, outputs: &mut Vec<OutputPart>) {
    match content.get("type").and_then(Value::as_str) {
        Some("image") => {
            let source = if let Some(data) = content.get("data").and_then(Value::as_str) {
                Some(AssetSource::Base64 { data: data.into() })
            } else {
                content
                    .get("uri")
                    .and_then(Value::as_str)
                    .map(|url| AssetSource::Url { url: url.into() })
            };
            if let Some(source) = source {
                outputs.push(OutputPart::Image {
                    source,
                    mime_type: content
                        .get("mime_type")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                    revised_prompt: None,
                    remote_file: None,
                });
            }
        }
        Some("text") => {
            if let Some(text) = content.get("text").and_then(Value::as_str) {
                outputs.push(OutputPart::Text {
                    text: text.into(),
                    annotations: content
                        .get("annotations")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default(),
                });
            }
        }
        _ => {}
    }
}

fn parse_grounding(metadata: Option<&Value>, outputs: &mut Vec<OutputPart>) {
    let Some(metadata) = metadata else {
        return;
    };
    for chunk in metadata
        .get("groundingChunks")
        .or_else(|| metadata.get("grounding_chunks"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let source = chunk.get("web").unwrap_or(chunk);
        outputs.push(OutputPart::Citation {
            title: source
                .get("title")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            url: source
                .get("uri")
                .or_else(|| source.get("url"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            snippet: None,
            raw: chunk.clone(),
        });
    }
    if let Some(html) = metadata
        .pointer("/searchEntryPoint/renderedContent")
        .or_else(|| metadata.pointer("/search_entry_point/rendered_content"))
        .and_then(Value::as_str)
    {
        outputs.push(OutputPart::SearchSuggestions {
            html: html.into(),
            signature: None,
        });
    }
}

async fn consume_generate_content_sse(
    request: &GenerationRequest,
    mut response: reqwest::Response,
    events: EventHandler,
) -> Result<GenerationResponse, ProviderError> {
    events(RunEvent::Started {
        request_id: request.request_id.clone(),
    });
    let mut buffer = String::new();
    let mut outputs = Vec::new();
    let mut raw_events = Vec::new();
    let mut usage = None;
    let mut model = None;
    while let Some(chunk) = response.chunk().await? {
        buffer.push_str(&String::from_utf8_lossy(&chunk).replace("\r\n", "\n"));
        while let Some(boundary) = buffer.find("\n\n") {
            let block = buffer[..boundary].to_owned();
            buffer.drain(..boundary + 2);
            let data = block
                .lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .map(str::trim_start)
                .collect::<Vec<_>>()
                .join("\n");
            if data.is_empty() || data == "[DONE]" {
                continue;
            }
            let value: Value = serde_json::from_str(&data)?;
            let parsed = normalize_generate_content_response(value.clone(), None)?;
            model = parsed.model.or(model);
            usage = parsed.usage.or(usage);
            for output in parsed.outputs {
                match &output {
                    OutputPart::Image { source, .. } => events(RunEvent::PartialImage {
                        index: outputs.len() as u32,
                        image: source.clone(),
                        raw: value.clone(),
                    }),
                    OutputPart::Text { text, .. } => events(RunEvent::TextDelta {
                        text: text.clone(),
                        raw: value.clone(),
                    }),
                    _ => events(RunEvent::Progress {
                        status: "gemini_stream_part".into(),
                        raw: value.clone(),
                    }),
                }
                outputs.push(output);
            }
            raw_events.push(value);
        }
    }
    if outputs.is_empty() {
        return Err(ProviderError::parse(
            "Gemini stream ended without output parts",
            Some(Value::Array(raw_events)),
        ));
    }
    let generation = GenerationResponse {
        provider: ProviderKind::Gemini,
        model,
        created_at: None,
        outputs,
        usage,
        remote_job: None,
        request_id: None,
        raw: Value::Array(raw_events),
    };
    events(RunEvent::Completed {
        response: generation.clone(),
    });
    Ok(generation)
}

async fn consume_interactions_sse(
    request: &GenerationRequest,
    mut response: reqwest::Response,
    events: EventHandler,
) -> Result<GenerationResponse, ProviderError> {
    events(RunEvent::Started {
        request_id: request.request_id.clone(),
    });
    let mut buffer = String::new();
    let mut raw_events = Vec::new();
    let mut accumulated_outputs = Vec::new();
    let mut interaction_id = None;
    let mut model = Some(request.model.clone());
    let mut terminal_status = RemoteJobStatus::Unknown;
    let mut completed_interaction = None;

    while let Some(chunk) = response.chunk().await? {
        buffer.push_str(&String::from_utf8_lossy(&chunk).replace("\r\n", "\n"));
        while let Some(boundary) = buffer.find("\n\n") {
            let block = buffer[..boundary].to_owned();
            buffer.drain(..boundary + 2);
            let sse_event = block
                .lines()
                .find_map(|line| line.strip_prefix("event:"))
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let data = block
                .lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .map(str::trim_start)
                .collect::<Vec<_>>()
                .join("\n");
            if data.is_empty() || data == "[DONE]" {
                continue;
            }
            let value: Value = serde_json::from_str(&data)?;
            let event_type = value
                .get("event_type")
                .or_else(|| value.get("type"))
                .and_then(Value::as_str)
                .or(sse_event)
                .unwrap_or("interaction.event")
                .to_owned();
            let event_id = value
                .get("event_id")
                .or_else(|| value.get("id").filter(|_| event_type.contains("event")))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            if let Some(event_id) = event_id {
                events(RunEvent::Checkpoint {
                    event_id,
                    status: value
                        .get("status")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                    raw: value.clone(),
                });
            }

            match event_type.as_str() {
                "interaction.created" | "interaction.start" => {
                    let interaction = value.get("interaction").unwrap_or(&value);
                    interaction_id = interaction
                        .get("id")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                        .or(interaction_id);
                    model = interaction
                        .get("model")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                        .or(model);
                    terminal_status = interaction
                        .get("status")
                        .and_then(Value::as_str)
                        .map(map_gemini_status)
                        .unwrap_or(RemoteJobStatus::Running);
                    events(RunEvent::Progress {
                        status: "interaction.created".into(),
                        raw: value.clone(),
                    });
                }
                "interaction.status_update"
                | "interaction.status.updated"
                | "interaction.status" => {
                    let status = value
                        .get("status")
                        .or_else(|| value.pointer("/interaction/status"))
                        .and_then(Value::as_str)
                        .unwrap_or("in_progress");
                    terminal_status = map_gemini_status(status);
                    events(RunEvent::Progress {
                        status: status.into(),
                        raw: value.clone(),
                    });
                }
                "step.delta" => {
                    if let Some(delta) = value.get("delta") {
                        parse_interaction_delta(delta, &value, &events, &mut accumulated_outputs);
                    }
                }
                "step.completed" | "step.done" => {
                    if let Some(step) = value.get("step") {
                        parse_interaction_step(step, &mut accumulated_outputs);
                    }
                    events(RunEvent::Progress {
                        status: event_type.clone(),
                        raw: value.clone(),
                    });
                }
                "interaction.completed" | "interaction.complete" => {
                    terminal_status = RemoteJobStatus::Succeeded;
                    let interaction = value.get("interaction").unwrap_or(&value);
                    interaction_id = interaction
                        .get("id")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                        .or(interaction_id);
                    completed_interaction = Some(interaction.clone());
                }
                "interaction.failed" | "interaction.error" | "error" => {
                    let message = value
                        .pointer("/error/message")
                        .or_else(|| value.get("message"))
                        .and_then(Value::as_str)
                        .unwrap_or("Gemini interaction stream failed");
                    let mut error = ProviderError::new(ProviderErrorKind::Api, message);
                    error.code = value.pointer("/error/code").and_then(|code| {
                        code.as_str()
                            .map(ToOwned::to_owned)
                            .or_else(|| Some(code.to_string()))
                    });
                    error.details = Some(value);
                    return Err(error);
                }
                "interaction.cancelled" => {
                    terminal_status = RemoteJobStatus::Cancelled;
                    events(RunEvent::Progress {
                        status: "cancelled".into(),
                        raw: value.clone(),
                    });
                }
                _ => events(RunEvent::Progress {
                    status: event_type,
                    raw: value.clone(),
                }),
            }
            raw_events.push(value);
        }
    }

    let mut generation = if let Some(interaction) = completed_interaction {
        normalize_interaction_response(interaction, None)?
    } else {
        GenerationResponse {
            provider: ProviderKind::Gemini,
            model,
            created_at: None,
            outputs: Vec::new(),
            usage: None,
            remote_job: interaction_id.clone().and_then(|id| {
                matches!(
                    terminal_status,
                    RemoteJobStatus::Queued | RemoteJobStatus::Running
                )
                .then(|| RemoteJob {
                    id,
                    kind: RemoteJobKind::Background,
                    status: terminal_status,
                    provider: ProviderKind::Gemini,
                    model: Some(request.model.clone()),
                    raw: Value::Array(raw_events.clone()),
                })
            }),
            request_id: None,
            raw: Value::Array(raw_events.clone()),
        }
    };
    if generation.outputs.is_empty() {
        generation.outputs = accumulated_outputs;
    }
    if generation.raw.is_null() {
        generation.raw = Value::Array(raw_events);
    }
    if generation.outputs.is_empty()
        && !matches!(
            terminal_status,
            RemoteJobStatus::Queued | RemoteJobStatus::Running
        )
    {
        return Err(ProviderError::parse(
            "Gemini interaction stream ended without output or a resumable background job",
            Some(generation.raw),
        ));
    }
    events(RunEvent::Completed {
        response: generation.clone(),
    });
    Ok(generation)
}

fn parse_interaction_delta(
    delta: &Value,
    event: &Value,
    events: &EventHandler,
    outputs: &mut Vec<OutputPart>,
) {
    if let Some(deltas) = delta.as_array() {
        for delta in deltas {
            parse_interaction_delta(delta, event, events, outputs);
        }
        return;
    }
    match delta.get("type").and_then(Value::as_str) {
        Some("text") => {
            if let Some(text) = delta.get("text").and_then(Value::as_str) {
                events(RunEvent::TextDelta {
                    text: text.into(),
                    raw: event.clone(),
                });
                outputs.push(OutputPart::Text {
                    text: text.into(),
                    annotations: delta
                        .get("annotations")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default(),
                });
            }
        }
        Some("image") => {
            let source = if let Some(data) = delta.get("data").and_then(Value::as_str) {
                Some(AssetSource::Base64 { data: data.into() })
            } else {
                delta
                    .get("uri")
                    .and_then(Value::as_str)
                    .map(|url| AssetSource::Url { url: url.into() })
            };
            if let Some(source) = source {
                events(RunEvent::PartialImage {
                    index: outputs.len() as u32,
                    image: source.clone(),
                    raw: event.clone(),
                });
                outputs.push(OutputPart::Image {
                    source,
                    mime_type: delta
                        .get("mime_type")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                    revised_prompt: None,
                    remote_file: None,
                });
            }
        }
        Some("thought") => {
            if let Some(text) = delta
                .get("text")
                .or_else(|| delta.get("summary"))
                .and_then(Value::as_str)
            {
                outputs.push(OutputPart::Thought {
                    text: text.into(),
                    signature: delta
                        .get("signature")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                });
            }
        }
        _ => {
            if let Some(content) = delta.get("content") {
                if let Some(items) = content.as_array() {
                    for item in items {
                        parse_interaction_content(item, outputs);
                    }
                } else {
                    parse_interaction_content(content, outputs);
                }
            }
        }
    }
}

fn parse_interaction_step(step: &Value, outputs: &mut Vec<OutputPart>) {
    match step.get("type").and_then(Value::as_str) {
        Some("model_output") => {
            for content in step
                .get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                parse_interaction_content(content, outputs);
            }
        }
        Some("thought") => {
            let text = step
                .get("summary")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n");
            if !text.is_empty() {
                outputs.push(OutputPart::Thought {
                    text,
                    signature: step
                        .get("signature")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                });
            }
        }
        _ => {}
    }
}

fn parse_gemini_job(value: Value, kind: RemoteJobKind) -> Result<RemoteJob, ProviderError> {
    let id = value
        .get("name")
        .or_else(|| value.get("id"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ProviderError::parse("Gemini job response has no name", Some(value.clone()))
        })?
        .to_owned();
    let status = value
        .get("state")
        .or_else(|| value.get("status"))
        .and_then(Value::as_str)
        .map(map_gemini_status)
        .unwrap_or(RemoteJobStatus::Queued);
    Ok(RemoteJob {
        id,
        kind,
        status,
        provider: ProviderKind::Gemini,
        model: value
            .get("model")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        raw: value,
    })
}

fn map_gemini_status(status: &str) -> RemoteJobStatus {
    match status.to_ascii_uppercase().as_str() {
        "PENDING" | "QUEUED" | "JOB_STATE_PENDING" | "JOB_STATE_QUEUED" => RemoteJobStatus::Queued,
        "RUNNING" | "IN_PROGRESS" | "REQUIRES_ACTION" | "JOB_STATE_RUNNING" => {
            RemoteJobStatus::Running
        }
        "SUCCEEDED" | "COMPLETED" | "JOB_STATE_SUCCEEDED" => RemoteJobStatus::Succeeded,
        "FAILED" | "JOB_STATE_FAILED" => RemoteJobStatus::Failed,
        "CANCELLED" | "CANCELED" | "JOB_STATE_CANCELLED" => RemoteJobStatus::Cancelled,
        "EXPIRED" | "JOB_STATE_EXPIRED" => RemoteJobStatus::Expired,
        _ => RemoteJobStatus::Unknown,
    }
}

fn parse_gemini_usage(value: &Value) -> UsageRecord {
    UsageRecord {
        input_tokens: value
            .get("promptTokenCount")
            .or_else(|| value.get("input_tokens"))
            .and_then(Value::as_u64),
        output_tokens: value
            .get("candidatesTokenCount")
            .or_else(|| value.get("output_tokens"))
            .and_then(Value::as_u64),
        total_tokens: value
            .get("totalTokenCount")
            .or_else(|| value.get("total_tokens"))
            .and_then(Value::as_u64),
        image_tokens: value
            .get("imageTokenCount")
            .or_else(|| value.get("image_tokens"))
            .and_then(Value::as_u64),
        cost_usd: value.get("cost_usd").and_then(Value::as_f64),
        raw: Some(value.clone()),
    }
}

fn inline_input_size(inputs: &[InputAsset]) -> Result<u64, ProviderError> {
    inputs.iter().try_fold(0_u64, |total, input| {
        let size = match input {
            InputAsset::LocalFile { path, .. } => {
                std::fs::metadata(path).map_err(ProviderError::io)?.len()
            }
            InputAsset::Base64 { data, .. } => {
                let encoded = data.split_once(',').map(|(_, data)| data).unwrap_or(data);
                ((encoded.len() as u64) * 3) / 4
            }
            InputAsset::Url { .. } | InputAsset::FileId { .. } => 0,
        };
        Ok(total.saturating_add(size))
    })
}

fn plain_base64(data: &str) -> Result<String, ProviderError> {
    if data.starts_with("data:") {
        let decoded = decode_base64_asset(data)?;
        Ok(base64::engine::general_purpose::STANDARD.encode(decoded.bytes))
    } else {
        base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|error| ProviderError::parse(error.to_string(), None))?;
        Ok(data.into())
    }
}

fn merge_extra(body: &mut Map<String, Value>, extra: &Map<String, Value>) {
    for (name, value) in extra {
        if !matches!(
            name.as_str(),
            "model" | "input" | "contents" | "response_format"
        ) {
            body.insert(name.clone(), value.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::{OutputSpec, ProviderOptions};
    use super::*;
    use std::sync::Arc;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn request(model: &str) -> GenerationRequest {
        GenerationRequest {
            request_id: "request".into(),
            model: model.into(),
            resolved_capability: None,
            operation: Operation::Generate,
            execution: ExecutionMode::Realtime,
            prompt: "draw a banana".into(),
            inputs: Vec::new(),
            mask: None,
            output: OutputSpec {
                count: 1,
                aspect_ratio: Some("16:9".into()),
                resolution: Some("1K".into()),
                ..OutputSpec::default()
            },
            options: ProviderOptions {
                gemini: Some(GeminiOptions::default()),
                ..ProviderOptions::default()
            },
        }
    }

    #[test]
    fn lite_rejects_non_1k_resolution() {
        let mut request = request("gemini-3.1-flash-lite-image");
        request.output.resolution = Some("2K".into());
        assert!(validate_gemini_request(&request).is_err());
    }

    #[test]
    fn video_input_requires_31_flash_and_explicit_mode() {
        let mut request = request("gemini-3.1-flash-image");
        request.operation = Operation::VideoReferenceToImage;
        request.inputs.push(InputAsset::FileId {
            id: "files/video".into(),
            mime_type: Some("video/mp4".into()),
            label: None,
        });
        assert!(validate_gemini_request(&request).is_ok());
        request.model = "gemini-3-pro-image".into();
        assert!(validate_gemini_request(&request).is_err());
    }

    #[test]
    fn interactions_body_exposes_image_output_controls() {
        let request = request("gemini-3.1-flash-image");
        let body = build_interactions_body(&request, &GeminiOptions::default()).unwrap();
        assert_eq!(
            body.pointer("/response_format/aspect_ratio"),
            Some(&json!("16:9"))
        );
        assert_eq!(
            body.pointer("/response_format/image_size"),
            Some(&json!("1K"))
        );
    }

    #[test]
    fn interactions_body_places_thinking_in_generation_config() {
        let request = request("gemini-3.1-flash-image");
        let options = GeminiOptions {
            thinking_level: Some("minimal".into()),
            include_thoughts: true,
            ..GeminiOptions::default()
        };
        let body = build_interactions_body(&request, &options).unwrap();
        assert_eq!(
            body.pointer("/generation_config/thinking_level"),
            Some(&json!("minimal"))
        );
        assert_eq!(
            body.pointer("/generation_config/thinking_summaries"),
            Some(&json!("auto"))
        );
    }

    #[test]
    fn temperature_and_top_p_stay_inside_each_generation_config() {
        let request = request("gemini-3.1-flash-image");
        let options = GeminiOptions {
            temperature: Some(0.7),
            top_p: Some(0.8),
            ..GeminiOptions::default()
        };
        let interactions = build_interactions_body(&request, &options).unwrap();
        assert_eq!(
            interactions.pointer("/generation_config/temperature"),
            Some(&json!(0.7))
        );
        assert_eq!(
            interactions.pointer("/generation_config/top_p"),
            Some(&json!(0.8))
        );
        assert!(interactions.get("temperature").is_none());

        let generate_content = build_generate_content_body(&request, &options).unwrap();
        assert_eq!(
            generate_content.pointer("/generationConfig/temperature"),
            Some(&json!(0.7))
        );
        assert_eq!(
            generate_content.pointer("/generationConfig/topP"),
            Some(&json!(0.8))
        );
        assert!(generate_content.get("topP").is_none());
    }

    #[tokio::test]
    async fn streams_interactions_and_normalizes_completed_image() {
        let server = MockServer::start().await;
        let stream = concat!(
            "event: interaction.created\n",
            "data: {\"event_type\":\"interaction.created\",\"event_id\":\"1\",\"interaction\":{\"id\":\"interaction-1\",\"status\":\"in_progress\",\"model\":\"gemini-3.1-flash-image\"}}\n\n",
            "event: interaction.status_update\n",
            "data: {\"event_type\":\"interaction.status_update\",\"event_id\":\"2\",\"status\":\"in_progress\"}\n\n",
            "event: step.delta\n",
            "data: {\"event_type\":\"step.delta\",\"event_id\":\"3\",\"delta\":{\"type\":\"text\",\"text\":\"rendering\"}}\n\n",
            "event: interaction.completed\n",
            "data: {\"event_type\":\"interaction.completed\",\"event_id\":\"4\",\"interaction\":{\"id\":\"interaction-1\",\"status\":\"completed\",\"model\":\"gemini-3.1-flash-image\",\"steps\":[{\"type\":\"model_output\",\"content\":[{\"type\":\"image\",\"data\":\"YWJj\",\"mime_type\":\"image/png\"}]}]}}\n\n"
        );
        Mock::given(method("POST"))
            .and(path("/interactions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(stream),
            )
            .mount(&server)
            .await;

        let mut config = ProviderConfig::gemini("test", "Gemini test");
        config.base_url = server.uri();
        let adapter = GeminiAdapter::new(
            config,
            ProviderCredentials {
                api_key: "test-key".into(),
            },
        )
        .unwrap();
        let response = adapter
            .execute_stream(&request("gemini-3.1-flash-image"), Arc::new(|_| {}))
            .await
            .unwrap();
        assert_eq!(response.outputs.len(), 1);
        assert!(matches!(response.outputs[0], OutputPart::Image { .. }));
    }

    #[tokio::test]
    async fn deletes_remote_file_name_without_request_body() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/files/file-123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
        let mut config = ProviderConfig::gemini("test", "Gemini test");
        config.base_url = server.uri();
        let adapter = GeminiAdapter::new(
            config,
            ProviderCredentials {
                api_key: "test-key".into(),
            },
        )
        .unwrap();

        adapter.delete_file("files/file-123").await.unwrap();

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].body.is_empty());
    }

    #[test]
    fn parses_interleaved_generate_content_parts() {
        let value = json!({
            "candidates": [{
                "content": {"parts": [
                    {"text": "Here is the image."},
                    {"inlineData": {"mimeType": "image/png", "data": "YWJj"}}
                ]}
            }]
        });
        let response = normalize_generate_content_response(value, None).unwrap();
        assert_eq!(response.outputs.len(), 2);
    }
}
