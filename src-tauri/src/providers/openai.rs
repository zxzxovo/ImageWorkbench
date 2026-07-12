use std::collections::HashSet;
use std::time::Instant;

use reqwest::header::CONTENT_TYPE;
use reqwest::{Method, RequestBuilder};
use serde_json::{Map, Value, json};

use super::capabilities::find_model;
use super::error::{ProviderError, ProviderErrorKind};
use super::http::{
    HttpTransport, decode_base64_asset, encode_path_segment, input_to_data_uri, input_to_reference,
    parse_error_response,
};
use super::types::{
    AssetSource, BatchSubmission, ConnectionTest, DiscoveredModel, DownloadedAsset, EventHandler,
    ExecutionMode, GenerationRequest, GenerationResponse, InputAsset, JobPollResult,
    OpenAiApiSurface, OpenAiOptions, Operation, OutputPart, ProviderConfig, ProviderCredentials,
    ProviderKind, RemoteJob, RemoteJobKind, RemoteJobStatus, RunEvent, UsageRecord,
};
use super::{ProviderAdapter, ProviderFuture};

pub struct OpenAiAdapter {
    transport: HttpTransport,
    kind: ProviderKind,
}

impl OpenAiAdapter {
    pub fn new(
        config: ProviderConfig,
        credentials: ProviderCredentials,
    ) -> Result<Self, ProviderError> {
        Self::new_with_kind(config, credentials, ProviderKind::OpenAi)
    }

    pub(crate) fn new_with_kind(
        config: ProviderConfig,
        credentials: ProviderCredentials,
        kind: ProviderKind,
    ) -> Result<Self, ProviderError> {
        Ok(Self {
            transport: HttpTransport::new(config, credentials)?,
            kind,
        })
    }

    fn options<'a>(&self, request: &'a GenerationRequest) -> OpenAiOptions {
        request
            .options
            .openai
            .clone()
            .unwrap_or_else(|| OpenAiOptions {
                api_surface: if request.operation == Operation::ConversationContinue {
                    OpenAiApiSurface::Responses
                } else {
                    OpenAiApiSurface::Images
                },
                ..OpenAiOptions::default()
            })
    }

    async fn execute_images(
        &self,
        request: &GenerationRequest,
        stream: bool,
    ) -> Result<(RequestBuilder, Option<String>), ProviderError> {
        let options = self.options(request);
        match request.operation {
            Operation::Generate => {
                let mut body = build_generation_body(request, &options);
                if stream {
                    body.insert("stream".into(), Value::Bool(true));
                }
                let builder = self
                    .openai_request(Method::POST, "/images/generations")?
                    .json(&body);
                Ok((builder, request.output.format.clone()))
            }
            Operation::Edit => {
                if request.model == "dall-e-2" {
                    let form = build_legacy_edit_form(request, &options, stream)?;
                    Ok((
                        self.openai_request(Method::POST, "/images/edits")?
                            .multipart(form),
                        None,
                    ))
                } else {
                    let mut body = build_edit_body(request, &options)?;
                    if stream {
                        body.insert("stream".into(), Value::Bool(true));
                    }
                    Ok((
                        self.openai_request(Method::POST, "/images/edits")?
                            .json(&body),
                        request.output.format.clone(),
                    ))
                }
            }
            Operation::Variation => Ok((
                self.openai_request(Method::POST, "/images/variations")?
                    .multipart(build_variation_form(request, &options)?),
                None,
            )),
            Operation::ConversationContinue | Operation::VideoReferenceToImage => {
                Err(ProviderError::unsupported(
                    "this operation is not supported by the OpenAI Images API",
                ))
            }
        }
    }

    async fn execute_responses(
        &self,
        request: &GenerationRequest,
    ) -> Result<GenerationResponse, ProviderError> {
        let options = self.options(request);
        if options.stream {
            return Err(ProviderError::unsupported(
                "Responses API streaming must be invoked through execute_stream",
            ));
        }
        let body = build_responses_body(request, &options)?;
        let response = self
            .transport
            .send_json(self.openai_request(Method::POST, "/responses")?.json(&body))
            .await?;
        normalize_responses_response(self.kind, response.value, response.request_id)
    }

    fn openai_request(&self, method: Method, path: &str) -> Result<RequestBuilder, ProviderError> {
        let mut request = self.transport.request(method, path)?;
        if let Some(organization) = &self.transport.config().organization {
            request = request.header("OpenAI-Organization", organization);
        }
        if let Some(project) = &self.transport.config().project {
            request = request.header("OpenAI-Project", project);
        }
        Ok(request)
    }

    async fn upload_batch_jsonl(
        &self,
        submission: &BatchSubmission,
    ) -> Result<String, ProviderError> {
        if submission.requests.is_empty() {
            return Err(ProviderError::validation("batch requests cannot be empty"));
        }
        let mut jsonl = String::new();
        for item in &submission.requests {
            let line = json!({
                "custom_id": item.key,
                "method": "POST",
                "url": normalize_openai_batch_endpoint(&item.endpoint),
                "body": item.body,
            });
            jsonl.push_str(&serde_json::to_string(&line)?);
            jsonl.push('\n');
        }
        let file_part = reqwest::multipart::Part::bytes(jsonl.into_bytes())
            .file_name("imageworkbench-batch.jsonl")
            .mime_str("application/jsonl")?;
        let response = self
            .transport
            .send_json(
                self.openai_request(Method::POST, "/files")?.multipart(
                    reqwest::multipart::Form::new()
                        .text("purpose", "batch")
                        .part("file", file_part),
                ),
            )
            .await?;
        response
            .value
            .get("id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or_else(|| {
                ProviderError::parse(
                    "OpenAI file upload response has no id",
                    Some(response.value),
                )
            })
    }

    async fn download_batch_outputs(
        &self,
        batch: &Value,
    ) -> Result<Vec<GenerationResponse>, ProviderError> {
        let Some(file_id) = batch.get("output_file_id").and_then(Value::as_str) else {
            return Ok(Vec::new());
        };
        let response = self
            .openai_request(Method::GET, &format!("/files/{file_id}/content"))?
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(parse_error_response(response).await);
        }
        let bytes = response.bytes().await?;
        let text = String::from_utf8(bytes.to_vec()).map_err(|error| {
            ProviderError::parse(
                format!("OpenAI batch output is not UTF-8 JSONL: {error}"),
                None,
            )
        })?;
        let mut outputs = Vec::new();
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let item: Value = serde_json::from_str(line)?;
            let status = item
                .pointer("/response/status_code")
                .and_then(Value::as_u64)
                .unwrap_or(200);
            if !(200..300).contains(&status) {
                continue;
            }
            let Some(body) = item.pointer("/response/body") else {
                continue;
            };
            let request_id = item
                .pointer("/response/request_id")
                .or_else(|| item.get("custom_id"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned);
            let generation = if body.get("data").and_then(Value::as_array).is_some() {
                normalize_images_response(
                    self.kind,
                    body.get("model")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                    body.get("output_format").and_then(Value::as_str),
                    body.clone(),
                    request_id,
                )?
            } else {
                normalize_responses_response(self.kind, body.clone(), request_id)?
            };
            outputs.push(generation);
        }
        Ok(outputs)
    }
}

impl ProviderAdapter for OpenAiAdapter {
    fn kind(&self) -> ProviderKind {
        self.kind
    }

    fn config(&self) -> &ProviderConfig {
        self.transport.config()
    }

    fn test_connection(&self) -> ProviderFuture<'_, ConnectionTest> {
        Box::pin(async move {
            let started = Instant::now();
            let models = self.list_models().await?;
            Ok(ConnectionTest {
                provider: self.kind,
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
                .unwrap_or("/models");
            let response = self
                .transport
                .send_json(self.openai_request(Method::GET, path)?)
                .await?;
            let models = response
                .value
                .get("data")
                .and_then(Value::as_array)
                .or_else(|| response.value.get("models").and_then(Value::as_array))
                .ok_or_else(|| {
                    ProviderError::parse(
                        "model response has no data array",
                        Some(response.value.clone()),
                    )
                })?;
            Ok(models
                .iter()
                .filter_map(|model| {
                    let id = model
                        .get("id")
                        .or_else(|| model.get("name"))?
                        .as_str()?
                        .to_owned();
                    Some(DiscoveredModel {
                        id,
                        display_name: model
                            .get("display_name")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned),
                        owned_by: model
                            .get("owned_by")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned),
                        supported_actions: Vec::new(),
                        raw: model.clone(),
                    })
                })
                .collect())
        })
    }

    fn validate(&self, request: &GenerationRequest) -> Result<(), ProviderError> {
        validate_openai_request(request, self.kind)
    }

    fn execute<'a>(
        &'a self,
        request: &'a GenerationRequest,
    ) -> ProviderFuture<'a, GenerationResponse> {
        Box::pin(async move {
            self.validate(request)?;
            if request.execution == ExecutionMode::ProviderBatch {
                return Err(ProviderError::unsupported(
                    "submit provider batches through submit_batch",
                ));
            }
            let options = self.options(request);
            if options.api_surface == OpenAiApiSurface::Responses
                || request.operation == Operation::ConversationContinue
            {
                return self.execute_responses(request).await;
            }
            if options.stream {
                return Err(ProviderError::unsupported(
                    "streaming requests must be invoked through execute_stream",
                ));
            }
            let (builder, format) = self.execute_images(request, false).await?;
            let response = self.transport.send_json(builder).await?;
            normalize_images_response(
                self.kind,
                Some(request.model.clone()),
                format.as_deref(),
                response.value,
                response.request_id,
            )
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
            if options.api_surface == OpenAiApiSurface::Responses {
                let mut body = build_responses_body(request, &options)?;
                body["stream"] = Value::Bool(true);
                let response = self
                    .openai_request(Method::POST, "/responses")?
                    .json(&body)
                    .send()
                    .await?;
                if !response.status().is_success() {
                    return Err(parse_error_response(response).await);
                }
                return consume_responses_sse(self.kind, request, response, events).await;
            }
            if request.operation == Operation::Variation {
                return Err(ProviderError::unsupported(
                    "DALL-E 2 variations do not support streaming",
                ));
            }
            let (builder, format) = self.execute_images(request, true).await?;
            let response = builder.send().await?;
            if !response.status().is_success() {
                return Err(parse_error_response(response).await);
            }
            consume_image_sse(self.kind, request, format.as_deref(), response, events).await
        })
    }

    fn submit_batch<'a>(
        &'a self,
        submission: &'a BatchSubmission,
    ) -> ProviderFuture<'a, RemoteJob> {
        Box::pin(async move {
            let input_file_id = match &submission.input_file_id {
                Some(id) => id.clone(),
                None => self.upload_batch_jsonl(submission).await?,
            };
            let endpoints: HashSet<String> = submission
                .requests
                .iter()
                .map(|item| normalize_openai_batch_endpoint(&item.endpoint))
                .collect();
            if endpoints.len() > 1 {
                return Err(ProviderError::validation(
                    "an OpenAI batch may target only one endpoint",
                ));
            }
            let endpoint = endpoints
                .into_iter()
                .next()
                .unwrap_or_else(|| "/v1/responses".into());
            let mut body = json!({
                "input_file_id": input_file_id,
                "endpoint": endpoint,
                "completion_window": submission.completion_window.as_deref().unwrap_or("24h"),
            });
            if !submission.metadata.is_empty() {
                body["metadata"] = Value::Object(submission.metadata.clone());
            }
            let response = self
                .transport
                .send_json(self.openai_request(Method::POST, "/batches")?.json(&body))
                .await?;
            parse_openai_job(self.kind, RemoteJobKind::Batch, response.value)
        })
    }

    fn poll_job<'a>(&'a self, job: &'a RemoteJob) -> ProviderFuture<'a, JobPollResult> {
        Box::pin(async move {
            let path = match job.kind {
                RemoteJobKind::Batch => format!("/batches/{}", job.id),
                RemoteJobKind::Background => format!("/responses/{}", job.id),
            };
            let response = self
                .transport
                .send_json(self.openai_request(Method::GET, &path)?)
                .await?;
            if job.kind == RemoteJobKind::Background {
                let generation = normalize_responses_response(
                    self.kind,
                    response.value.clone(),
                    response.request_id,
                )?;
                let remote_job = generation.remote_job.clone().unwrap_or(RemoteJob {
                    id: job.id.clone(),
                    kind: job.kind,
                    status: RemoteJobStatus::Succeeded,
                    provider: self.kind,
                    model: generation.model.clone(),
                    raw: response.value,
                });
                return Ok(JobPollResult {
                    job: remote_job,
                    outputs: vec![generation],
                    next_page_token: None,
                });
            }
            let raw = response.value;
            let remote_job = parse_openai_job(self.kind, job.kind, raw.clone())?;
            let outputs = if remote_job.status == RemoteJobStatus::Succeeded {
                self.download_batch_outputs(&raw).await?
            } else {
                Vec::new()
            };
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
                RemoteJobKind::Batch => format!("/batches/{}/cancel", job.id),
                RemoteJobKind::Background => format!("/responses/{}/cancel", job.id),
            };
            let response = self
                .transport
                .send_json(self.openai_request(Method::POST, &path)?)
                .await?;
            parse_openai_job(self.kind, job.kind, response.value)
        })
    }

    fn delete_file<'a>(&'a self, file_id: &'a str) -> ProviderFuture<'a, ()> {
        Box::pin(async move {
            let file_id = encode_path_segment(file_id)?;
            self.transport
                .send_json(self.openai_request(Method::DELETE, &format!("/files/{file_id}"))?)
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
                let response = self
                    .openai_request(Method::GET, &format!("/files/{id}/content"))?
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
                    filename: Some(id.clone()),
                });
            }
            self.transport.download(source).await
        })
    }
}

pub(crate) fn validate_openai_request(
    request: &GenerationRequest,
    kind: ProviderKind,
) -> Result<(), ProviderError> {
    if request.model.trim().is_empty() {
        return Err(ProviderError::validation("model is required"));
    }
    if request.operation != Operation::Variation && request.prompt.trim().is_empty() {
        return Err(ProviderError::validation("prompt is required"));
    }
    let options = request.options.openai.clone().unwrap_or_default();
    let is_compatible = kind == ProviderKind::OpenAiCompatible;
    let catalog_capability = if request.resolved_capability.is_some() {
        None
    } else {
        find_model(
            if is_compatible {
                ProviderKind::OpenAiCompatible
            } else {
                ProviderKind::OpenAi
            },
            &request.model,
        )?
    };
    let capability = request
        .resolved_capability
        .as_deref()
        .or(catalog_capability);
    if let Some(capability) = capability {
        if !capability.operations.contains(&request.operation)
            && options.api_surface == OpenAiApiSurface::Images
        {
            return Err(ProviderError::validation(format!(
                "{} does not support {:?}",
                request.model, request.operation
            )));
        }
        if let Some(max_chars) = capability.prompt_max_chars {
            if request.prompt.chars().count() > max_chars as usize {
                return Err(ProviderError::validation(format!(
                    "prompt exceeds the {max_chars}-character model limit"
                )));
            }
        }
        if let Some(limit) = &capability.output_count {
            let count = u32::from(request.output.count);
            if count < limit.min || count > limit.max {
                return Err(ProviderError::validation(format!(
                    "output count must be between {} and {}",
                    limit.min, limit.max
                )));
            }
        }
        if let Some(max_images) = capability.max_input_images {
            if request.inputs.len() > usize::from(max_images) {
                return Err(ProviderError::validation(format!(
                    "{} accepts at most {max_images} input images",
                    request.model
                )));
            }
        }
        if let Some(size) = &request.output.size {
            validate_openai_size(size, capability)?;
        }
        validate_allowed(
            "quality",
            request.output.quality.as_deref(),
            &capability.qualities,
        )?;
        validate_allowed(
            "format",
            request.output.format.as_deref(),
            &capability.formats,
        )?;
        validate_allowed(
            "response format",
            request.output.response_format.as_deref(),
            &capability.response_formats,
        )?;
        validate_allowed(
            "background",
            request.output.background.as_deref(),
            &capability.backgrounds,
        )?;
        if request.mask.is_some() && !capability.features.mask {
            return Err(ProviderError::validation(format!(
                "{} does not support mask editing",
                capability.id
            )));
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
    } else if !is_compatible && options.api_surface == OpenAiApiSurface::Images {
        return Err(ProviderError::validation(format!(
            "unknown OpenAI image model {}; refresh the capability catalog or add an override",
            request.model
        )));
    }

    match request.operation {
        Operation::Generate => {
            if !request.inputs.is_empty() || request.mask.is_some() {
                return Err(ProviderError::validation(
                    "generation requests cannot contain input images or a mask",
                ));
            }
        }
        Operation::Edit => {
            if request.inputs.is_empty() {
                return Err(ProviderError::validation(
                    "image editing requires at least one input image",
                ));
            }
        }
        Operation::Variation => {
            if request.inputs.len() != 1 {
                return Err(ProviderError::validation(
                    "variation requires exactly one input image",
                ));
            }
            if request.mask.is_some() {
                return Err(ProviderError::validation(
                    "variation does not accept a mask",
                ));
            }
        }
        Operation::ConversationContinue => {
            if options.api_surface != OpenAiApiSurface::Responses {
                return Err(ProviderError::validation(
                    "conversation continuation requires the Responses API surface",
                ));
            }
        }
        Operation::VideoReferenceToImage => {
            return Err(ProviderError::unsupported(
                "OpenAI image generation does not accept video input",
            ));
        }
    }

    if request.output.background.as_deref() == Some("transparent")
        && !matches!(
            request.output.format.as_deref(),
            None | Some("png") | Some("webp")
        )
    {
        return Err(ProviderError::validation(
            "transparent output requires png or webp",
        ));
    }
    if request.output.compression.is_some()
        && !matches!(
            request.output.format.as_deref(),
            Some("jpeg") | Some("webp")
        )
    {
        return Err(ProviderError::validation(
            "output compression is only valid for jpeg or webp",
        ));
    }
    if let Some(partial_images) = options.partial_images {
        if partial_images > 3 {
            return Err(ProviderError::validation(
                "partial_images must be between 0 and 3",
            ));
        }
        if !options.stream {
            return Err(ProviderError::validation(
                "partial_images requires stream=true",
            ));
        }
    }
    let capability_model = capability
        .map(|capability| capability.id.as_str())
        .unwrap_or(&request.model);
    if options.style.is_some() && !is_compatible && capability_model != "dall-e-3" {
        return Err(ProviderError::validation(
            "style is only supported by dall-e-3",
        ));
    }
    if capability_model.starts_with("gpt-image-2")
        && options
            .input_fidelity
            .as_deref()
            .is_some_and(|value| value != "high")
    {
        return Err(ProviderError::validation(
            "gpt-image-2 edits always use high input fidelity",
        ));
    }
    if request.execution == ExecutionMode::Background
        && options.api_surface != OpenAiApiSurface::Responses
    {
        return Err(ProviderError::validation(
            "background execution is only available through the Responses API",
        ));
    }
    Ok(())
}

fn validate_openai_size(
    size: &str,
    capability: &super::capabilities::ModelCapability,
) -> Result<(), ProviderError> {
    if capability.sizes.iter().any(|allowed| allowed == size) {
        return Ok(());
    }
    let Some(rule) = &capability.custom_size else {
        return Err(ProviderError::validation(format!(
            "size {size} is not supported by {}",
            capability.id
        )));
    };
    let (width, height) = parse_dimensions(size)?;
    if width % rule.multiple_of != 0 || height % rule.multiple_of != 0 {
        return Err(ProviderError::validation(format!(
            "custom dimensions must be divisible by {}",
            rule.multiple_of
        )));
    }
    let ratio = f64::from(width) / f64::from(height);
    if ratio < rule.min_aspect_ratio || ratio > rule.max_aspect_ratio {
        return Err(ProviderError::validation(
            "custom aspect ratio must be between 1:3 and 3:1",
        ));
    }
    if width > rule.max_edge
        || height > rule.max_edge
        || u64::from(width) * u64::from(height) > rule.max_pixels
    {
        return Err(ProviderError::validation(
            "custom dimensions exceed the model limit",
        ));
    }
    Ok(())
}

fn parse_dimensions(value: &str) -> Result<(u32, u32), ProviderError> {
    let (width, height) = value
        .split_once('x')
        .ok_or_else(|| ProviderError::validation("size must use WIDTHxHEIGHT"))?;
    let width = width
        .parse::<u32>()
        .map_err(|_| ProviderError::validation("width must be a positive integer"))?;
    let height = height
        .parse::<u32>()
        .map_err(|_| ProviderError::validation("height must be a positive integer"))?;
    if width == 0 || height == 0 {
        return Err(ProviderError::validation("dimensions must be positive"));
    }
    Ok((width, height))
}

fn validate_allowed(
    field: &str,
    value: Option<&str>,
    allowed: &[String],
) -> Result<(), ProviderError> {
    if let Some(value) = value {
        if allowed.is_empty() || !allowed.iter().any(|allowed| allowed == value) {
            return Err(ProviderError::validation(format!(
                "{field} {value} is not supported"
            )));
        }
    }
    Ok(())
}

pub(crate) fn build_generation_body(
    request: &GenerationRequest,
    options: &OpenAiOptions,
) -> Map<String, Value> {
    let mut body = Map::new();
    body.insert("model".into(), Value::String(request.model.clone()));
    body.insert("prompt".into(), Value::String(request.prompt.clone()));
    body.insert("n".into(), Value::from(request.output.count));
    insert_string(&mut body, "size", request.output.size.as_deref());
    insert_string(&mut body, "quality", request.output.quality.as_deref());
    insert_string(&mut body, "output_format", request.output.format.as_deref());
    insert_string(
        &mut body,
        "response_format",
        request.output.response_format.as_deref(),
    );
    insert_string(
        &mut body,
        "background",
        request.output.background.as_deref(),
    );
    if let Some(compression) = request.output.compression {
        body.insert("output_compression".into(), Value::from(compression));
    }
    insert_string(&mut body, "moderation", options.moderation.as_deref());
    insert_string(&mut body, "style", options.style.as_deref());
    insert_string(&mut body, "user", options.user.as_deref());
    if options.stream {
        body.insert("stream".into(), Value::Bool(true));
    }
    if let Some(partial_images) = options.partial_images {
        body.insert("partial_images".into(), Value::from(partial_images));
    }
    merge_extra(&mut body, &options.extra);
    body
}

pub(crate) fn build_edit_body(
    request: &GenerationRequest,
    options: &OpenAiOptions,
) -> Result<Map<String, Value>, ProviderError> {
    let mut body = build_generation_body(request, options);
    let images = request
        .inputs
        .iter()
        .map(input_to_reference)
        .collect::<Result<Vec<_>, _>>()?;
    body.insert("images".into(), Value::Array(images));
    if let Some(mask) = &request.mask {
        body.insert("mask".into(), input_to_reference(mask)?);
    }
    insert_string(
        &mut body,
        "input_fidelity",
        options.input_fidelity.as_deref(),
    );
    Ok(body)
}

fn build_responses_body(
    request: &GenerationRequest,
    options: &OpenAiOptions,
) -> Result<Value, ProviderError> {
    let mut content = vec![json!({ "type": "input_text", "text": request.prompt })];
    for input in &request.inputs {
        let item = match input {
            InputAsset::FileId { id, .. } => json!({ "type": "input_image", "file_id": id }),
            _ => json!({ "type": "input_image", "image_url": input_to_data_uri(input)? }),
        };
        content.push(item);
    }
    let mut tool = Map::new();
    tool.insert("type".into(), Value::String("image_generation".into()));
    insert_string(&mut tool, "model", options.image_model.as_deref());
    insert_string(&mut tool, "size", request.output.size.as_deref());
    insert_string(&mut tool, "quality", request.output.quality.as_deref());
    insert_string(&mut tool, "output_format", request.output.format.as_deref());
    insert_string(
        &mut tool,
        "background",
        request.output.background.as_deref(),
    );
    insert_string(
        &mut tool,
        "input_fidelity",
        options.input_fidelity.as_deref(),
    );
    insert_string(
        &mut tool,
        "action",
        options.image_generation_action.as_deref(),
    );
    if let Some(compression) = request.output.compression {
        tool.insert("output_compression".into(), Value::from(compression));
    }
    if let Some(partial_images) = options.partial_images {
        tool.insert("partial_images".into(), Value::from(partial_images));
    }
    let mut body = json!({
        "model": request.model,
        "input": [{ "role": "user", "content": content }],
        "tools": [Value::Object(tool)],
        "tool_choice": { "type": "image_generation" },
    });
    if let Some(previous) = options.previous_response_id.as_deref() {
        body["previous_response_id"] = Value::String(previous.into());
    }
    if request.execution == ExecutionMode::Background {
        body["background"] = Value::Bool(true);
    }
    if !request.options.extra.is_empty() {
        if let Some(object) = body.as_object_mut() {
            merge_extra(object, &request.options.extra);
        }
    }
    Ok(body)
}

fn build_legacy_edit_form(
    request: &GenerationRequest,
    options: &OpenAiOptions,
    stream: bool,
) -> Result<reqwest::multipart::Form, ProviderError> {
    let mut form = reqwest::multipart::Form::new()
        .text("model", request.model.clone())
        .text("prompt", request.prompt.clone())
        .text("n", request.output.count.to_string());
    for input in &request.inputs {
        form = form.part("image", multipart_image(input, "image.png")?);
    }
    if let Some(mask) = &request.mask {
        form = form.part("mask", multipart_image(mask, "mask.png")?);
    }
    form = add_common_form_fields(form, request, options);
    if stream {
        form = form.text("stream", "true");
    }
    Ok(form)
}

fn build_variation_form(
    request: &GenerationRequest,
    options: &OpenAiOptions,
) -> Result<reqwest::multipart::Form, ProviderError> {
    let mut form = reqwest::multipart::Form::new()
        .part("image", multipart_image(&request.inputs[0], "image.png")?)
        .text("model", request.model.clone())
        .text("n", request.output.count.to_string());
    form = add_common_form_fields(form, request, options);
    Ok(form)
}

fn add_common_form_fields(
    mut form: reqwest::multipart::Form,
    request: &GenerationRequest,
    options: &OpenAiOptions,
) -> reqwest::multipart::Form {
    for (name, value) in [
        ("size", request.output.size.as_deref()),
        ("quality", request.output.quality.as_deref()),
        ("response_format", request.output.response_format.as_deref()),
        ("background", request.output.background.as_deref()),
        ("output_format", request.output.format.as_deref()),
        ("moderation", options.moderation.as_deref()),
        ("user", options.user.as_deref()),
        ("input_fidelity", options.input_fidelity.as_deref()),
    ] {
        if let Some(value) = value {
            form = form.text(name, value.to_owned());
        }
    }
    if let Some(compression) = request.output.compression {
        form = form.text("output_compression", compression.to_string());
    }
    if let Some(partial_images) = options.partial_images {
        form = form.text("partial_images", partial_images.to_string());
    }
    form
}

fn multipart_image(
    input: &InputAsset,
    fallback_name: &str,
) -> Result<reqwest::multipart::Part, ProviderError> {
    let (bytes, mime_type, filename) = match input {
        InputAsset::LocalFile {
            path, mime_type, ..
        } => (
            std::fs::read(path).map_err(ProviderError::io)?,
            mime_type.clone(),
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(fallback_name)
                .to_owned(),
        ),
        InputAsset::Base64 {
            data, mime_type, ..
        } => {
            let decoded = decode_base64_asset(data)?;
            (decoded.bytes, mime_type.clone(), fallback_name.to_owned())
        }
        InputAsset::Url { .. } | InputAsset::FileId { .. } => {
            return Err(ProviderError::validation(
                "this legacy multipart endpoint requires a local or base64 input",
            ));
        }
    };
    Ok(reqwest::multipart::Part::bytes(bytes)
        .file_name(filename)
        .mime_str(&mime_type)?)
}

pub(crate) fn normalize_images_response(
    kind: ProviderKind,
    model: Option<String>,
    requested_format: Option<&str>,
    value: Value,
    request_id: Option<String>,
) -> Result<GenerationResponse, ProviderError> {
    let data = value.get("data").and_then(Value::as_array).ok_or_else(|| {
        ProviderError::parse("image response has no data array", Some(value.clone()))
    })?;
    let response_format = value
        .get("output_format")
        .and_then(Value::as_str)
        .or(requested_format);
    let mime_type = response_format.map(output_format_to_mime);
    let outputs = data
        .iter()
        .filter_map(|image| {
            let source = if let Some(data) = image.get("b64_json").and_then(Value::as_str) {
                Some(AssetSource::Base64 { data: data.into() })
            } else {
                image
                    .get("url")
                    .and_then(Value::as_str)
                    .map(|url| AssetSource::Url { url: url.into() })
            }?;
            let remote_file = image.get("file_output").map(parse_remote_file);
            Some(OutputPart::Image {
                source,
                mime_type: mime_type.clone(),
                revised_prompt: image
                    .get("revised_prompt")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                remote_file,
            })
        })
        .collect::<Vec<_>>();
    if outputs.is_empty() && !data.is_empty() {
        return Err(ProviderError::parse(
            "image response contains neither URL nor base64 data",
            Some(value),
        ));
    }
    Ok(GenerationResponse {
        provider: kind,
        model,
        created_at: value.get("created").and_then(Value::as_i64),
        outputs,
        usage: value.get("usage").map(parse_usage),
        remote_job: None,
        request_id,
        raw: value,
    })
}

fn normalize_responses_response(
    kind: ProviderKind,
    value: Value,
    request_id: Option<String>,
) -> Result<GenerationResponse, ProviderError> {
    let status = value
        .get("status")
        .and_then(Value::as_str)
        .map(map_status)
        .unwrap_or(RemoteJobStatus::Unknown);
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let mut outputs = Vec::new();
    if let Some(items) = value.get("output").and_then(Value::as_array) {
        for item in items {
            parse_responses_output_item(item, &mut outputs);
        }
    }
    let remote_job = if matches!(status, RemoteJobStatus::Queued | RemoteJobStatus::Running) {
        id.clone().map(|id| RemoteJob {
            id,
            kind: RemoteJobKind::Background,
            status,
            provider: kind,
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
        provider: kind,
        model: value
            .get("model")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        created_at: value.get("created_at").and_then(Value::as_i64),
        outputs,
        usage: value.get("usage").map(parse_usage),
        remote_job,
        request_id,
        raw: value,
    })
}

fn parse_responses_output_item(item: &Value, outputs: &mut Vec<OutputPart>) {
    match item.get("type").and_then(Value::as_str) {
        Some("image_generation_call") => {
            if let Some(data) = item
                .get("result")
                .or_else(|| item.get("b64_json"))
                .and_then(Value::as_str)
            {
                outputs.push(OutputPart::Image {
                    source: AssetSource::Base64 { data: data.into() },
                    mime_type: item
                        .get("output_format")
                        .and_then(Value::as_str)
                        .map(output_format_to_mime),
                    revised_prompt: item
                        .get("revised_prompt")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                    remote_file: None,
                });
            }
        }
        Some("message") => {
            for content in item
                .get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(text) = content
                    .get("text")
                    .or_else(|| content.get("output_text"))
                    .and_then(Value::as_str)
                {
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
        }
        _ => {}
    }
}

async fn consume_responses_sse(
    kind: ProviderKind,
    request: &GenerationRequest,
    mut response: reqwest::Response,
    events: EventHandler,
) -> Result<GenerationResponse, ProviderError> {
    events(RunEvent::Started {
        request_id: request.request_id.clone(),
    });
    let mut buffer = String::new();
    let mut raw_events = Vec::new();
    let mut completed_response = None;
    let mut text_delta = String::new();
    let mut completed_items = Vec::new();

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
                .get("type")
                .and_then(Value::as_str)
                .or(sse_event)
                .unwrap_or("response.event")
                .to_owned();
            match event_type.as_str() {
                "response.created" | "response.in_progress" | "response.queued" => {
                    events(RunEvent::Progress {
                        status: event_type.clone(),
                        raw: value.clone(),
                    });
                }
                "response.image_generation_call.partial_image" => {
                    if let Some(image) = value
                        .get("partial_image_b64")
                        .or_else(|| value.get("b64_json"))
                        .or_else(|| value.get("delta"))
                        .and_then(Value::as_str)
                    {
                        events(RunEvent::PartialImage {
                            index: value
                                .get("partial_image_index")
                                .and_then(Value::as_u64)
                                .unwrap_or_default() as u32,
                            image: AssetSource::Base64 { data: image.into() },
                            raw: value.clone(),
                        });
                    }
                }
                "response.output_text.delta" => {
                    if let Some(delta) = value.get("delta").and_then(Value::as_str) {
                        text_delta.push_str(delta);
                        events(RunEvent::TextDelta {
                            text: delta.into(),
                            raw: value.clone(),
                        });
                    }
                }
                "response.output_item.done" | "response.output_item.completed" => {
                    if let Some(item) = value.get("item") {
                        parse_responses_output_item(item, &mut completed_items);
                    }
                }
                "response.completed" => {
                    completed_response = Some(value.get("response").unwrap_or(&value).clone());
                }
                "response.failed" | "response.error" | "error" => {
                    let error_value = value
                        .get("error")
                        .or_else(|| value.pointer("/response/error"))
                        .unwrap_or(&value);
                    let message = error_value
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("OpenAI Responses stream failed");
                    let mut error = ProviderError::new(ProviderErrorKind::Api, message);
                    error.code = error_value.get("code").and_then(|code| {
                        code.as_str()
                            .map(ToOwned::to_owned)
                            .or_else(|| Some(code.to_string()))
                    });
                    error.details = Some(value);
                    return Err(error);
                }
                _ => events(RunEvent::Progress {
                    status: event_type,
                    raw: value.clone(),
                }),
            }
            raw_events.push(value);
        }
    }

    let completed = completed_response.ok_or_else(|| {
        ProviderError::parse(
            "OpenAI Responses stream ended without response.completed",
            Some(Value::Array(raw_events.clone())),
        )
    })?;
    let mut generation = normalize_responses_response(kind, completed, None)?;
    if generation.outputs.is_empty() {
        generation.outputs = completed_items;
        if !text_delta.is_empty()
            && !generation
                .outputs
                .iter()
                .any(|output| matches!(output, OutputPart::Text { .. }))
        {
            generation.outputs.push(OutputPart::Text {
                text: text_delta,
                annotations: Vec::new(),
            });
        }
    }
    if generation.outputs.is_empty() {
        return Err(ProviderError::parse(
            "OpenAI Responses stream completed without image or text output",
            Some(Value::Array(raw_events)),
        ));
    }
    events(RunEvent::Completed {
        response: generation.clone(),
    });
    Ok(generation)
}

async fn consume_image_sse(
    kind: ProviderKind,
    request: &GenerationRequest,
    format: Option<&str>,
    mut response: reqwest::Response,
    events: EventHandler,
) -> Result<GenerationResponse, ProviderError> {
    events(RunEvent::Started {
        request_id: request.request_id.clone(),
    });
    let mut buffer = String::new();
    let mut completed: Option<Value> = None;
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
            let event_type = value
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if event_type.ends_with("partial_image") {
                if let Some(image) = value.get("b64_json").and_then(Value::as_str) {
                    events(RunEvent::PartialImage {
                        index: value
                            .get("partial_image_index")
                            .and_then(Value::as_u64)
                            .unwrap_or_default() as u32,
                        image: AssetSource::Base64 { data: image.into() },
                        raw: value,
                    });
                }
            } else if event_type.ends_with("completed") {
                completed = Some(value);
            } else {
                events(RunEvent::Progress {
                    status: event_type.into(),
                    raw: value,
                });
            }
        }
    }
    let completed = completed.ok_or_else(|| {
        ProviderError::parse("stream ended without a completed image event", None)
    })?;
    let image = completed
        .get("b64_json")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ProviderError::parse(
                "completed stream event has no image",
                Some(completed.clone()),
            )
        })?;
    let generation = GenerationResponse {
        provider: kind,
        model: Some(request.model.clone()),
        created_at: None,
        outputs: vec![OutputPart::Image {
            source: AssetSource::Base64 { data: image.into() },
            mime_type: format.map(output_format_to_mime),
            revised_prompt: None,
            remote_file: None,
        }],
        usage: completed.get("usage").map(parse_usage),
        remote_job: None,
        request_id: None,
        raw: completed,
    };
    events(RunEvent::Completed {
        response: generation.clone(),
    });
    Ok(generation)
}

fn parse_openai_job(
    kind: ProviderKind,
    job_kind: RemoteJobKind,
    value: Value,
) -> Result<RemoteJob, ProviderError> {
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| ProviderError::parse("remote job response has no id", Some(value.clone())))?
        .to_owned();
    Ok(RemoteJob {
        id,
        kind: job_kind,
        status: value
            .get("status")
            .and_then(Value::as_str)
            .map(map_status)
            .unwrap_or(RemoteJobStatus::Unknown),
        provider: kind,
        model: value
            .get("model")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        raw: value,
    })
}

fn map_status(status: &str) -> RemoteJobStatus {
    match status.to_ascii_lowercase().as_str() {
        "queued" | "validating" | "in_progress" | "finalizing" | "cancelling" => {
            RemoteJobStatus::Running
        }
        "completed" | "succeeded" => RemoteJobStatus::Succeeded,
        "failed" => RemoteJobStatus::Failed,
        "cancelled" | "canceled" => RemoteJobStatus::Cancelled,
        "expired" => RemoteJobStatus::Expired,
        _ => RemoteJobStatus::Unknown,
    }
}

fn parse_usage(value: &Value) -> UsageRecord {
    UsageRecord {
        input_tokens: value
            .get("input_tokens")
            .or_else(|| value.get("prompt_tokens"))
            .and_then(Value::as_u64),
        output_tokens: value
            .get("output_tokens")
            .or_else(|| value.get("completion_tokens"))
            .and_then(Value::as_u64),
        total_tokens: value.get("total_tokens").and_then(Value::as_u64),
        image_tokens: value
            .pointer("/output_tokens_details/image_tokens")
            .or_else(|| value.pointer("/input_tokens_details/image_tokens"))
            .and_then(Value::as_u64),
        cost_usd: value.get("cost_in_usd").and_then(Value::as_f64),
        raw: Some(value.clone()),
    }
}

fn parse_remote_file(value: &Value) -> super::types::RemoteFile {
    super::types::RemoteFile {
        id: value
            .get("file_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .into(),
        filename: value
            .get("filename")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        expires_at: value.get("expires_at").cloned(),
        public_url: value
            .get("public_url")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        public_url_expires_at: value.get("public_url_expires_at").cloned(),
        public_url_error: value
            .get("public_url_error")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
    }
}

fn output_format_to_mime(format: &str) -> String {
    match format.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg".into(),
        "webp" => "image/webp".into(),
        _ => "image/png".into(),
    }
}

fn normalize_openai_batch_endpoint(endpoint: &str) -> String {
    let endpoint = endpoint.trim();
    if endpoint.starts_with("/v1/") {
        endpoint.into()
    } else {
        format!("/v1/{}", endpoint.trim_start_matches('/'))
    }
}

fn insert_string(body: &mut Map<String, Value>, name: &str, value: Option<&str>) {
    if let Some(value) = value {
        body.insert(name.into(), Value::String(value.into()));
    }
}

fn merge_extra(body: &mut Map<String, Value>, extra: &Map<String, Value>) {
    for (name, value) in extra {
        if !matches!(
            name.as_str(),
            "model" | "prompt" | "images" | "image" | "mask"
        ) {
            body.insert(name.clone(), value.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::{OutputSpec, ProviderOptions};
    use super::*;
    use std::sync::{Arc, Mutex};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn request(model: &str) -> GenerationRequest {
        GenerationRequest {
            request_id: "request".into(),
            model: model.into(),
            resolved_capability: None,
            operation: Operation::Generate,
            execution: ExecutionMode::Realtime,
            prompt: "draw a lighthouse".into(),
            inputs: Vec::new(),
            mask: None,
            output: OutputSpec {
                count: 1,
                ..OutputSpec::default()
            },
            options: ProviderOptions::default(),
        }
    }

    #[test]
    fn validates_gpt_image_2_custom_size() {
        let mut request = request("gpt-image-2");
        request.output.size = Some("1536x864".into());
        assert!(validate_openai_request(&request, ProviderKind::OpenAi).is_ok());
        request.output.size = Some("1537x864".into());
        assert!(validate_openai_request(&request, ProviderKind::OpenAi).is_err());
        request.output.size = Some("3840x512".into());
        assert!(validate_openai_request(&request, ProviderKind::OpenAi).is_err());
    }

    #[test]
    fn rejects_dall_e_3_multiple_outputs() {
        let mut request = request("dall-e-3");
        request.output.count = 2;
        assert!(validate_openai_request(&request, ProviderKind::OpenAi).is_err());
    }

    #[test]
    fn requires_transparency_capable_format() {
        let mut request = request("gpt-image-1.5");
        request.output.background = Some("transparent".into());
        request.output.format = Some("jpeg".into());
        assert!(validate_openai_request(&request, ProviderKind::OpenAi).is_err());
    }

    #[test]
    fn normalizes_url_and_base64_images() {
        let value = json!({
            "created": 1,
            "data": [
                {"url": "https://example.com/image.png"},
                {"b64_json": "YWJj"}
            ]
        });
        let response = normalize_images_response(
            ProviderKind::OpenAi,
            Some("gpt-image-1.5".into()),
            Some("png"),
            value,
            None,
        )
        .unwrap();
        assert_eq!(response.outputs.len(), 2);
    }

    #[tokio::test]
    async fn streams_responses_partial_images_text_and_completed_output() {
        let server = MockServer::start().await;
        let stream = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp-1\",\"status\":\"in_progress\",\"model\":\"gpt-5\"}}\n\n",
            "event: response.image_generation_call.partial_image\n",
            "data: {\"type\":\"response.image_generation_call.partial_image\",\"partial_image_index\":0,\"partial_image_b64\":\"UEFSVElBTA==\"}\n\n",
            "event: response.output_text.delta\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"working\"}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp-1\",\"status\":\"completed\",\"model\":\"gpt-5\",\"output\":[{\"type\":\"image_generation_call\",\"result\":\"RklOQUw=\",\"output_format\":\"png\"},{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"done\",\"annotations\":[]}]}]}}\n\n"
        );
        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(stream),
            )
            .mount(&server)
            .await;
        let mut config = ProviderConfig::openai("test", "OpenAI test");
        config.base_url = server.uri();
        let adapter = OpenAiAdapter::new(
            config,
            ProviderCredentials {
                api_key: "test-key".into(),
            },
        )
        .unwrap();
        let mut request = request("gpt-5");
        request.operation = Operation::ConversationContinue;
        request.options.openai = Some(OpenAiOptions {
            api_surface: OpenAiApiSurface::Responses,
            stream: true,
            image_model: Some("gpt-image-1.5".into()),
            ..OpenAiOptions::default()
        });
        let received = Arc::new(Mutex::new(Vec::new()));
        let sink = received.clone();
        let response = adapter
            .execute_stream(
                &request,
                Arc::new(move |event| sink.lock().unwrap().push(event)),
            )
            .await
            .unwrap();
        assert_eq!(response.outputs.len(), 2);
        let received = received.lock().unwrap();
        assert!(
            received
                .iter()
                .any(|event| matches!(event, RunEvent::PartialImage { .. }))
        );
        assert!(
            received
                .iter()
                .any(|event| matches!(event, RunEvent::TextDelta { .. }))
        );
    }

    #[tokio::test]
    async fn returns_responses_stream_failure_as_provider_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/responses"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(concat!(
                        "event: response.failed\n",
                        "data: {\"type\":\"response.failed\",\"response\":{\"error\":{\"code\":\"generation_failed\",\"message\":\"generation failed\"}}}\n\n"
                    )),
            )
            .mount(&server)
            .await;
        let mut config = ProviderConfig::openai("test", "OpenAI test");
        config.base_url = server.uri();
        let adapter = OpenAiAdapter::new(
            config,
            ProviderCredentials {
                api_key: "test-key".into(),
            },
        )
        .unwrap();
        let mut request = request("gpt-5");
        request.operation = Operation::ConversationContinue;
        request.options.openai = Some(OpenAiOptions {
            api_surface: OpenAiApiSurface::Responses,
            stream: true,
            ..OpenAiOptions::default()
        });
        let error = adapter
            .execute_stream(&request, Arc::new(|_| {}))
            .await
            .unwrap_err();
        assert_eq!(error.kind, ProviderErrorKind::Api);
        assert_eq!(error.code.as_deref(), Some("generation_failed"));
    }

    #[tokio::test]
    async fn deletes_remote_file_without_request_body() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/files/file-123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "deleted": true })))
            .expect(1)
            .mount(&server)
            .await;
        let mut config = ProviderConfig::openai("test", "OpenAI test");
        config.base_url = server.uri();
        let adapter = OpenAiAdapter::new(
            config,
            ProviderCredentials {
                api_key: "test-key".into(),
            },
        )
        .unwrap();

        adapter.delete_file("file-123").await.unwrap();

        let requests = server.received_requests().await.unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].body.is_empty());
    }
}
