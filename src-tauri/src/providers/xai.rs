use std::time::Instant;

use reqwest::Method;
use reqwest::header::CONTENT_TYPE;
use serde_json::{Map, Value, json};

use super::capabilities::find_model;
use super::error::ProviderError;
use super::http::{HttpTransport, encode_path_segment, input_to_data_uri, parse_error_response};
use super::openai::normalize_images_response;
use super::types::{
    AssetSource, BatchSubmission, ConnectionTest, DiscoveredModel, DownloadedAsset, ExecutionMode,
    GenerationRequest, GenerationResponse, JobPollResult, Operation, ProviderConfig,
    ProviderCredentials, ProviderKind, RemoteJob, RemoteJobKind, RemoteJobStatus, UsageRecord,
    XaiOptions, XaiPublicUrlOptions,
};
use super::{ProviderAdapter, ProviderFuture};

pub struct XaiAdapter {
    transport: HttpTransport,
}

impl XaiAdapter {
    pub fn new(
        config: ProviderConfig,
        credentials: ProviderCredentials,
    ) -> Result<Self, ProviderError> {
        Ok(Self {
            transport: HttpTransport::new(config, credentials)?,
        })
    }

    fn options(&self, request: &GenerationRequest) -> XaiOptions {
        request.options.xai.clone().unwrap_or_default()
    }

    async fn list_models_at(&self, path: &str) -> Result<Vec<DiscoveredModel>, ProviderError> {
        let response = self
            .transport
            .send_json(self.transport.request(Method::GET, path)?)
            .await?;
        let models = response
            .value
            .get("models")
            .and_then(Value::as_array)
            .or_else(|| response.value.get("data").and_then(Value::as_array))
            .ok_or_else(|| {
                ProviderError::parse(
                    "xAI model response has no models array",
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
                id.starts_with("grok-imagine-image")
                    .then(|| DiscoveredModel {
                        id,
                        display_name: model
                            .get("display_name")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned),
                        owned_by: model
                            .get("owned_by")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned),
                        supported_actions: vec!["generate".into(), "edit".into()],
                        raw: model.clone(),
                    })
            })
            .collect())
    }

    async fn create_inline_batch(
        &self,
        submission: &BatchSubmission,
    ) -> Result<RemoteJob, ProviderError> {
        if submission.requests.is_empty() {
            return Err(ProviderError::validation("batch requests cannot be empty"));
        }
        let created = self
            .transport
            .send_json(
                self.transport
                    .request(Method::POST, "/batches")?
                    .json(&json!({ "name": submission.name })),
            )
            .await?;
        let batch_id = created
            .value
            .get("batch_id")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ProviderError::parse(
                    "xAI batch response has no batch_id",
                    Some(created.value.clone()),
                )
            })?
            .to_owned();
        let batch_requests = submission
            .requests
            .iter()
            .map(|item| {
                let wrapper = xai_batch_wrapper(&item.endpoint)?;
                let mut batch_request = Map::new();
                batch_request.insert(wrapper.into(), item.body.clone());
                Ok(json!({
                    "batch_request_id": item.key,
                    "batch_request": Value::Object(batch_request)
                }))
            })
            .collect::<Result<Vec<Value>, ProviderError>>()?;
        self.transport
            .send_json(
                self.transport
                    .request(Method::POST, &format!("/batches/{batch_id}/requests"))?
                    .json(&json!({ "batch_requests": batch_requests })),
            )
            .await?;
        parse_xai_job(created.value)
    }

    async fn fetch_batch_results(
        &self,
        job: &RemoteJob,
    ) -> Result<(Vec<GenerationResponse>, Option<String>), ProviderError> {
        let mut outputs = Vec::new();
        let mut pagination_token: Option<String> = None;
        loop {
            let mut url = reqwest::Url::parse(
                &self
                    .transport
                    .endpoint(&format!("/batches/{}/results", job.id)),
            )
            .map_err(|error| ProviderError::validation(error.to_string()))?;
            {
                let mut query = url.query_pairs_mut();
                query.append_pair("limit", "1000");
                if let Some(token) = pagination_token.as_deref() {
                    query.append_pair("pagination_token", token);
                }
            }
            let response = self
                .transport
                .send_json(self.transport.request_url(Method::GET, url.as_str())?)
                .await?;
            for result in response
                .value
                .get("results")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let image = result
                    .pointer("/batch_result/response/image_generation")
                    .or_else(|| result.pointer("/batch_result/response/image_edit"));
                if let Some(image) = image {
                    outputs.push(normalize_images_response(
                        ProviderKind::Xai,
                        image
                            .get("model")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned),
                        None,
                        image.clone(),
                        result
                            .get("batch_request_id")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned),
                    )?);
                }
            }
            let next_token = response
                .value
                .get("pagination_token")
                .and_then(Value::as_str)
                .filter(|token| !token.is_empty())
                .map(ToOwned::to_owned);
            if next_token.is_some() && next_token == pagination_token {
                return Err(ProviderError::parse(
                    "xAI batch pagination repeated the same token",
                    Some(response.value),
                ));
            }
            pagination_token = next_token;
            if pagination_token.is_none() {
                break;
            }
        }
        Ok((outputs, None))
    }
}

impl ProviderAdapter for XaiAdapter {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Xai
    }

    fn config(&self) -> &ProviderConfig {
        self.transport.config()
    }

    fn test_connection(&self) -> ProviderFuture<'_, ConnectionTest> {
        Box::pin(async move {
            let started = Instant::now();
            let models = self.list_models().await?;
            Ok(ConnectionTest {
                provider: ProviderKind::Xai,
                latency_ms: started.elapsed().as_millis(),
                model_count: models.len(),
            })
        })
    }

    fn list_models(&self) -> ProviderFuture<'_, Vec<DiscoveredModel>> {
        Box::pin(async move {
            if let Some(path) = self.transport.config().models_path.as_deref() {
                return self.list_models_at(path).await;
            }
            match self.list_models_at("/image-generation-models").await {
                Ok(models) => Ok(models),
                Err(primary) => self.list_models_at("/models").await.map_err(|fallback| {
                    let mut error = fallback;
                    error.message = format!(
                        "xAI image model discovery failed at both endpoints: {}; {}",
                        primary.message, error.message
                    );
                    error
                }),
            }
        })
    }

    fn validate(&self, request: &GenerationRequest) -> Result<(), ProviderError> {
        validate_xai_request(request)
    }

    fn execute<'a>(
        &'a self,
        request: &'a GenerationRequest,
    ) -> ProviderFuture<'a, GenerationResponse> {
        Box::pin(async move {
            self.validate(request)?;
            if request.execution != ExecutionMode::Realtime {
                return Err(ProviderError::unsupported(
                    "xAI image requests use realtime execution; use submit_batch for provider batches",
                ));
            }
            let options = self.options(request);
            let body = build_xai_body(request, &options)?;
            let endpoint = match request.operation {
                Operation::Generate => "/images/generations",
                Operation::Edit => "/images/edits",
                _ => unreachable!("validated operation"),
            };
            let response = self
                .transport
                .send_json(self.transport.request(Method::POST, endpoint)?.json(&body))
                .await?;
            normalize_images_response(
                ProviderKind::Xai,
                response
                    .value
                    .get("model")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
                    .or_else(|| Some(request.model.clone())),
                None,
                response.value,
                response.request_id,
            )
        })
    }

    fn submit_batch<'a>(
        &'a self,
        submission: &'a BatchSubmission,
    ) -> ProviderFuture<'a, RemoteJob> {
        Box::pin(async move {
            if submission.input_file_id.is_none() {
                return self.create_inline_batch(submission).await;
            }
            let response = self
                .transport
                .send_json(
                    self.transport
                        .request(Method::POST, "/batches")?
                        .json(&json!({
                            "name": submission.name,
                            "input_file_id": submission.input_file_id,
                        })),
                )
                .await?;
            parse_xai_job(response.value)
        })
    }

    fn poll_job<'a>(&'a self, job: &'a RemoteJob) -> ProviderFuture<'a, JobPollResult> {
        Box::pin(async move {
            if job.kind != RemoteJobKind::Batch {
                return Err(ProviderError::unsupported(
                    "xAI image background jobs are not supported",
                ));
            }
            let response = self
                .transport
                .send_json(
                    self.transport
                        .request(Method::GET, &format!("/batches/{}", job.id))?,
                )
                .await?;
            let remote_job = parse_xai_job(response.value)?;
            let (outputs, next_page_token) = if remote_job.status == RemoteJobStatus::Succeeded {
                self.fetch_batch_results(&remote_job).await?
            } else {
                (Vec::new(), None)
            };
            Ok(JobPollResult {
                job: remote_job,
                outputs,
                next_page_token,
            })
        })
    }

    fn cancel_job<'a>(&'a self, job: &'a RemoteJob) -> ProviderFuture<'a, RemoteJob> {
        Box::pin(async move {
            if job.kind != RemoteJobKind::Batch {
                return Err(ProviderError::unsupported(
                    "xAI image background jobs are not supported",
                ));
            }
            let response = self
                .transport
                .send_json(
                    self.transport
                        .request(Method::POST, &format!("/batches/{}:cancel", job.id))?,
                )
                .await?;
            parse_xai_job(response.value)
        })
    }

    fn delete_file<'a>(&'a self, file_id: &'a str) -> ProviderFuture<'a, ()> {
        Box::pin(async move {
            let file_id = encode_path_segment(file_id)?;
            self.transport
                .send_json(
                    self.transport
                        .request(Method::DELETE, &format!("/files/{file_id}"))?,
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
                let response = self
                    .transport
                    .request(Method::GET, &format!("/files/{id}/content"))?
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

pub(crate) fn validate_xai_request(request: &GenerationRequest) -> Result<(), ProviderError> {
    if request.prompt.trim().is_empty() {
        return Err(ProviderError::validation("prompt is required"));
    }
    let catalog_capability = if request.resolved_capability.is_some() {
        None
    } else {
        find_model(ProviderKind::Xai, &request.model)?
    };
    let capability = request
        .resolved_capability
        .as_deref()
        .or(catalog_capability)
        .ok_or_else(|| {
            ProviderError::validation(format!(
                "unknown xAI image model {}; refresh model capabilities",
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
                "xAI output count must be between {} and {}",
                limit.min, limit.max
            )));
        }
    }
    match request.operation {
        Operation::Generate if !request.inputs.is_empty() => {
            return Err(ProviderError::validation(
                "xAI generation does not accept source images; use edit mode",
            ));
        }
        Operation::Edit if request.inputs.is_empty() => {
            return Err(ProviderError::validation(
                "xAI editing requires at least one input image",
            ));
        }
        _ => {}
    }
    if let Some(max_images) = capability.max_input_images
        && request.inputs.len() > usize::from(max_images)
    {
        return Err(ProviderError::validation(format!(
            "{} accepts at most {max_images} input images",
            capability.id
        )));
    }
    if request.inputs.len() > 1 && !capability.features.multiple_images {
        return Err(ProviderError::validation(format!(
            "{} does not support multiple input images",
            capability.id
        )));
    }
    if request.mask.is_some() && !capability.features.mask {
        return Err(ProviderError::unsupported(
            "xAI image editing does not support masks",
        ));
    }
    if request
        .inputs
        .iter()
        .any(|input| matches!(input, super::types::InputAsset::FileId { .. }))
        && !capability.features.file_inputs
    {
        return Err(ProviderError::validation(format!(
            "{} does not support provider file inputs",
            capability.id
        )));
    }
    if request.execution == super::types::ExecutionMode::Background
        && !capability.features.background
    {
        return Err(ProviderError::validation(format!(
            "{} does not support background execution",
            capability.id
        )));
    }
    if request.execution == super::types::ExecutionMode::ProviderBatch && !capability.features.batch
    {
        return Err(ProviderError::validation(format!(
            "{} does not support provider batches",
            capability.id
        )));
    }
    if let Some(aspect_ratio) = request.output.aspect_ratio.as_deref() {
        if !capability
            .aspect_ratios
            .iter()
            .any(|allowed| allowed == aspect_ratio)
        {
            return Err(ProviderError::validation("unsupported xAI aspect ratio"));
        }
    }
    if let Some(resolution) = request.output.resolution.as_deref() {
        if !capability
            .resolutions
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(resolution))
        {
            return Err(ProviderError::validation("xAI resolution must be 1k or 2k"));
        }
    }
    if let Some(response_format) = request.output.response_format.as_deref() {
        if !capability
            .response_formats
            .iter()
            .any(|allowed| allowed == response_format)
        {
            return Err(ProviderError::validation(
                "xAI response format must be url or b64_json",
            ));
        }
    }
    if request.output.size.is_some()
        || request.output.quality.is_some()
        || request.output.format.is_some()
        || request.output.background.is_some()
        || request.output.compression.is_some()
    {
        return Err(ProviderError::validation(
            "xAI uses aspect_ratio, resolution and model selection instead of size, quality, format or background",
        ));
    }
    if let Some(storage) = request
        .options
        .xai
        .as_ref()
        .and_then(|options| options.storage.as_ref())
    {
        if !capability.features.file_outputs {
            return Err(ProviderError::validation(format!(
                "{} does not support remote file storage",
                capability.id
            )));
        }
        if storage.filename.trim().is_empty() {
            return Err(ProviderError::validation(
                "xAI storage filename is required",
            ));
        }
        validate_expiry(storage.expires_after)?;
        if let Some(XaiPublicUrlOptions::Config { expires_after }) = &storage.public_url {
            validate_expiry(*expires_after)?;
            if let (Some(file_expiry), Some(url_expiry)) = (storage.expires_after, *expires_after) {
                if url_expiry > file_expiry {
                    return Err(ProviderError::validation(
                        "a public URL cannot outlive its stored file",
                    ));
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn build_xai_body(
    request: &GenerationRequest,
    options: &XaiOptions,
) -> Result<Map<String, Value>, ProviderError> {
    let mut body = Map::new();
    body.insert("model".into(), Value::String(request.model.clone()));
    body.insert("prompt".into(), Value::String(request.prompt.clone()));
    body.insert("n".into(), Value::from(request.output.count));
    if let Some(value) = &request.output.aspect_ratio {
        body.insert("aspect_ratio".into(), Value::String(value.clone()));
    }
    if let Some(value) = &request.output.resolution {
        body.insert(
            "resolution".into(),
            Value::String(value.to_ascii_lowercase()),
        );
    }
    if let Some(value) = &request.output.response_format {
        body.insert("response_format".into(), Value::String(value.clone()));
    }
    if let Some(storage) = &options.storage {
        body.insert("storage_options".into(), serde_json::to_value(storage)?);
    }
    if request.operation == Operation::Edit {
        let mut images = request
            .inputs
            .iter()
            .map(xai_image_reference)
            .collect::<Result<Vec<_>, _>>()?;
        if images.is_empty() {
            return Err(ProviderError::validation(
                "xAI edit payload requires at least one image",
            ));
        } else if images.len() == 1 {
            body.insert("image".into(), images.remove(0));
        } else {
            body.insert("images".into(), Value::Array(images));
        }
    }
    if let Some(mask) = &request.mask {
        body.insert("mask".into(), xai_image_reference(mask)?);
    }
    for (name, value) in &options.extra {
        if !matches!(name.as_str(), "model" | "prompt" | "image" | "images") {
            body.insert(name.clone(), value.clone());
        }
    }
    Ok(body)
}

fn xai_image_reference(input: &super::types::InputAsset) -> Result<Value, ProviderError> {
    match input {
        super::types::InputAsset::FileId { id, .. } => Ok(json!({ "file_id": id })),
        _ => Ok(json!({
            "url": input_to_data_uri(input)?,
            "type": "image_url"
        })),
    }
}

fn validate_expiry(expiry: Option<u32>) -> Result<(), ProviderError> {
    if let Some(expiry) = expiry {
        if !(3600..=2_592_000).contains(&expiry) {
            return Err(ProviderError::validation(
                "xAI file and public URL expiry must be between 3600 and 2592000 seconds",
            ));
        }
    }
    Ok(())
}

fn xai_batch_wrapper(endpoint: &str) -> Result<&'static str, ProviderError> {
    let endpoint = endpoint.trim_end_matches('/');
    if endpoint.ends_with("/images/generations") {
        Ok("image_generation")
    } else if endpoint.ends_with("/images/edits") {
        Ok("image_edit")
    } else {
        Err(ProviderError::validation(format!(
            "unsupported xAI image batch endpoint {endpoint}"
        )))
    }
}

fn parse_xai_job(value: Value) -> Result<RemoteJob, ProviderError> {
    let id = value
        .get("batch_id")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ProviderError::parse("xAI batch response has no batch_id", Some(value.clone()))
        })?
        .to_owned();
    let state = value.get("state").and_then(Value::as_object);
    let total = state
        .and_then(|state| state.get("num_requests"))
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let pending = state
        .and_then(|state| state.get("num_pending"))
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let cancelled = state
        .and_then(|state| state.get("num_cancelled"))
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let status = if value
        .get("cancel_time")
        .is_some_and(|value| !value.is_null())
    {
        RemoteJobStatus::Cancelled
    } else if total == 0 {
        RemoteJobStatus::Queued
    } else if pending > 0 {
        RemoteJobStatus::Running
    } else if cancelled == total {
        RemoteJobStatus::Cancelled
    } else {
        RemoteJobStatus::Succeeded
    };
    Ok(RemoteJob {
        id,
        kind: RemoteJobKind::Batch,
        status,
        provider: ProviderKind::Xai,
        model: None,
        raw: value,
    })
}

#[allow(dead_code)]
fn parse_xai_usage(value: &Value) -> UsageRecord {
    UsageRecord {
        input_tokens: value.get("input_tokens").and_then(Value::as_u64),
        output_tokens: value.get("output_tokens").and_then(Value::as_u64),
        total_tokens: value.get("total_tokens").and_then(Value::as_u64),
        image_tokens: value.get("image_tokens").and_then(Value::as_u64),
        cost_usd: value
            .get("cost_in_usd")
            .and_then(Value::as_f64)
            .or_else(|| {
                value
                    .get("cost_in_usd_ticks")
                    .and_then(Value::as_f64)
                    .map(|ticks| ticks / 10_000_000_000.0)
            }),
        raw: Some(value.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::{OutputSpec, ProviderOptions};
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn edit_request() -> GenerationRequest {
        GenerationRequest {
            request_id: "request".into(),
            model: "grok-imagine-image-quality".into(),
            resolved_capability: None,
            operation: Operation::Edit,
            execution: ExecutionMode::Realtime,
            prompt: "combine these images".into(),
            inputs: vec![super::super::types::InputAsset::Url {
                url: "https://example.com/one.png".into(),
                mime_type: Some("image/png".into()),
                label: None,
            }],
            mask: None,
            output: OutputSpec {
                count: 1,
                resolution: Some("2k".into()),
                ..OutputSpec::default()
            },
            options: ProviderOptions::default(),
        }
    }

    #[test]
    fn accepts_up_to_three_edit_images() {
        let mut request = edit_request();
        request.inputs.extend(request.inputs.clone());
        request.inputs.push(request.inputs[0].clone());
        assert!(validate_xai_request(&request).is_ok());
        request.inputs.push(request.inputs[0].clone());
        assert!(validate_xai_request(&request).is_err());
    }

    #[test]
    fn validates_storage_ttl() {
        let mut request = edit_request();
        request.options.xai = Some(XaiOptions {
            storage: Some(super::super::types::XaiStorageOptions {
                filename: "result.png".into(),
                expires_after: Some(3599),
                public_url: None,
            }),
            extra: Map::new(),
        });
        assert!(validate_xai_request(&request).is_err());
    }

    #[test]
    fn unknown_model_override_drives_xai_validation() {
        let mut request = edit_request();
        request.model = "future-image".into();
        request.resolved_capability = super::super::capabilities::resolve_model(
            ProviderKind::Xai,
            "future-image",
            Some(&serde_json::json!({
                "operations": ["edit"],
                "max_input_images": 2,
                "output_count": { "min": 1, "max": 2 },
                "resolutions": ["2k"],
                "features": { "multiple_images": true }
            })),
        )
        .unwrap()
        .map(Box::new);
        assert!(validate_xai_request(&request).is_ok());

        request.inputs.extend(request.inputs.clone());
        request.inputs.push(request.inputs[0].clone());
        assert!(validate_xai_request(&request).is_err());
    }

    #[test]
    fn builds_json_edit_payload() {
        let request = edit_request();
        let body = build_xai_body(&request, &XaiOptions::default()).unwrap();
        assert!(body.get("image").is_some());
        assert!(body.get("images").is_none());
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
        let mut config = ProviderConfig::xai("test", "xAI test");
        config.base_url = server.uri();
        let adapter = XaiAdapter::new(
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
