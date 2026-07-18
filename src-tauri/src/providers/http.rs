use std::time::Duration;

use base64::Engine;
use futures_util::StreamExt;
use reqwest::header::{CONTENT_TYPE, HeaderName, HeaderValue, RETRY_AFTER};
use reqwest::{Method, RequestBuilder, Response};
use serde_json::Value;

use super::error::{ProviderError, ProviderErrorKind};
use super::types::{
    AssetSource, AuthScheme, DownloadedAsset, InputAsset, ProviderConfig, ProviderCredentials,
};

const MAX_JSON_RESPONSE_BYTES: usize = 10 * 1024 * 1024;
const MAX_ERROR_RESPONSE_BYTES: usize = 10 * 1024 * 1024;
const MAX_BASE64_ASSET_BYTES: usize = 50 * 1024 * 1024;
const MAX_DOWNLOADED_ASSET_BYTES: usize = 100 * 1024 * 1024;
const MAX_LOCAL_ASSET_BYTES: u64 = 100 * 1024 * 1024;

#[derive(Clone)]
pub(crate) struct HttpTransport {
    client: reqwest::Client,
    config: ProviderConfig,
    credentials: ProviderCredentials,
}

pub(crate) struct JsonResponse {
    pub value: Value,
    pub request_id: Option<String>,
}

impl HttpTransport {
    pub fn new(
        config: ProviderConfig,
        credentials: ProviderCredentials,
    ) -> Result<Self, ProviderError> {
        validate_config(&config, &credentials)?;
        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs.max(1)))
            .user_agent(concat!("ImageWorkbench/", env!("CARGO_PKG_VERSION")));
        if let Some(proxy_url) = &config.proxy_url {
            builder = builder.proxy(reqwest::Proxy::all(proxy_url).map_err(|error| {
                ProviderError::new(ProviderErrorKind::Configuration, error.to_string())
            })?);
        }
        let client = builder.build()?;
        Ok(Self {
            client,
            config,
            credentials,
        })
    }

    pub fn config(&self) -> &ProviderConfig {
        &self.config
    }

    pub fn endpoint(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.config.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    pub fn request(&self, method: Method, path: &str) -> Result<RequestBuilder, ProviderError> {
        self.request_url(method, &self.endpoint(path))
    }

    pub fn request_url(&self, method: Method, url: &str) -> Result<RequestBuilder, ProviderError> {
        let mut request = self.client.request(method, url);
        request = match &self.config.auth {
            AuthScheme::Bearer => request.bearer_auth(&self.credentials.api_key),
            AuthScheme::Header { name, prefix } => {
                let name = HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
                    ProviderError::new(ProviderErrorKind::Configuration, error.to_string())
                })?;
                let value = prefix
                    .as_ref()
                    .map(|prefix| format!("{prefix}{}", self.credentials.api_key))
                    .unwrap_or_else(|| self.credentials.api_key.clone());
                request.header(name, value)
            }
            AuthScheme::Query { name } => {
                request.query(&[(name.as_str(), self.credentials.api_key.as_str())])
            }
        };

        for (name, value) in &self.config.headers {
            let header_name = HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
                ProviderError::new(ProviderErrorKind::Configuration, error.to_string())
            })?;
            let header_value = HeaderValue::from_str(value).map_err(|error| {
                ProviderError::new(ProviderErrorKind::Configuration, error.to_string())
            })?;
            request = request.header(header_name, header_value);
        }
        Ok(request)
    }

    pub async fn send_json(&self, request: RequestBuilder) -> Result<JsonResponse, ProviderError> {
        parse_json_response(request.send().await?).await
    }

    pub async fn download(&self, source: &AssetSource) -> Result<DownloadedAsset, ProviderError> {
        match source {
            AssetSource::Base64 { data } => decode_base64_asset(data),
            AssetSource::Url { url } => {
                let response = self.client.get(url).send().await?;
                let status = response.status();
                let mime_type = response
                    .headers()
                    .get(CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .map(ToOwned::to_owned);
                if !status.is_success() {
                    return Err(parse_error_response(response).await);
                }
                Ok(DownloadedAsset {
                    bytes: read_response_limited(
                        response,
                        MAX_DOWNLOADED_ASSET_BYTES,
                        "downloaded image",
                    )
                    .await?,
                    mime_type,
                    filename: filename_from_url(url),
                })
            }
            AssetSource::FileId { id } => Err(ProviderError::unsupported(format!(
                "file ID {id} requires a provider-specific download endpoint"
            ))),
        }
    }
}

pub(crate) async fn parse_json_response(response: Response) -> Result<JsonResponse, ProviderError> {
    if !response.status().is_success() {
        return Err(parse_error_response(response).await);
    }
    let request_id = request_id_from_headers(response.headers());
    let bytes = read_response_limited(response, MAX_JSON_RESPONSE_BYTES, "JSON response").await?;
    if bytes.is_empty() {
        return Ok(JsonResponse {
            value: Value::Null,
            request_id,
        });
    }
    let value = serde_json::from_slice(&bytes).map_err(|error| {
        ProviderError::parse(
            format!("provider returned invalid JSON: {error}"),
            Some(Value::String(String::from_utf8_lossy(&bytes).into_owned())),
        )
    })?;
    Ok(JsonResponse { value, request_id })
}

pub(crate) async fn parse_error_response(response: Response) -> ProviderError {
    let status = response.status();
    let headers = response.headers().clone();
    let bytes =
        match read_response_limited(response, MAX_ERROR_RESPONSE_BYTES, "error response").await {
            Ok(bytes) => bytes,
            Err(mut error) => {
                error.status = Some(status.as_u16());
                error.request_id = request_id_from_headers(&headers);
                return error;
            }
        };
    let parsed: Option<Value> = serde_json::from_slice(&bytes).ok();
    let message = parsed
        .as_ref()
        .and_then(api_error_message)
        .unwrap_or_else(|| String::from_utf8_lossy(&bytes).trim().to_owned())
        .trim()
        .to_owned();
    let kind = match status.as_u16() {
        401 => ProviderErrorKind::Authentication,
        403 => ProviderErrorKind::Permission,
        429 => ProviderErrorKind::RateLimit,
        _ => ProviderErrorKind::Api,
    };
    let mut error = ProviderError::new(
        kind,
        if message.is_empty() {
            format!("provider request failed with HTTP {status}")
        } else {
            message
        },
    );
    error.status = Some(status.as_u16());
    error.code = parsed.as_ref().and_then(api_error_code);
    error.request_id = request_id_from_headers(&headers);
    error.retry_after_seconds = headers
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok());
    error.details = parsed.or_else(|| {
        (!bytes.is_empty()).then(|| Value::String(String::from_utf8_lossy(&bytes).into_owned()))
    });
    error
}

pub(crate) fn input_to_data_uri(asset: &InputAsset) -> Result<String, ProviderError> {
    match asset {
        InputAsset::Url { url, .. } => Ok(url.clone()),
        InputAsset::Base64 {
            data, mime_type, ..
        } => {
            if data.starts_with("data:") {
                Ok(data.clone())
            } else {
                Ok(format!("data:{mime_type};base64,{data}"))
            }
        }
        InputAsset::FileId { id, .. } => Err(ProviderError::validation(format!(
            "file ID {id} cannot be represented as a data URI"
        ))),
        InputAsset::LocalFile {
            path, mime_type, ..
        } => {
            let size = std::fs::metadata(path).map_err(ProviderError::io)?.len();
            if size > MAX_LOCAL_ASSET_BYTES {
                return Err(payload_too_large(
                    "local input image",
                    size,
                    MAX_LOCAL_ASSET_BYTES,
                ));
            }
            let bytes = std::fs::read(path).map_err(ProviderError::io)?;
            Ok(format!(
                "data:{mime_type};base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            ))
        }
    }
}

pub(crate) fn input_to_reference(asset: &InputAsset) -> Result<Value, ProviderError> {
    match asset {
        InputAsset::FileId { id, .. } => Ok(serde_json::json!({ "file_id": id })),
        _ => Ok(serde_json::json!({ "image_url": input_to_data_uri(asset)? })),
    }
}

pub(crate) fn decode_base64_asset(data: &str) -> Result<DownloadedAsset, ProviderError> {
    let (mime_type, encoded) = if let Some((metadata, encoded)) = data.split_once(',') {
        if metadata.starts_with("data:") && metadata.ends_with(";base64") {
            (
                Some(
                    metadata
                        .trim_start_matches("data:")
                        .trim_end_matches(";base64")
                        .to_owned(),
                ),
                encoded,
            )
        } else {
            (None, data)
        }
    } else {
        (None, data)
    };
    let estimated_size = encoded.len().saturating_mul(3) / 4;
    if estimated_size > MAX_BASE64_ASSET_BYTES {
        return Err(payload_too_large(
            "base64 image",
            u64::try_from(estimated_size).unwrap_or(u64::MAX),
            u64::try_from(MAX_BASE64_ASSET_BYTES).unwrap_or(u64::MAX),
        ));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| ProviderError::parse(error.to_string(), None))?;
    if bytes.len() > MAX_BASE64_ASSET_BYTES {
        return Err(payload_too_large(
            "base64 image",
            u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            u64::try_from(MAX_BASE64_ASSET_BYTES).unwrap_or(u64::MAX),
        ));
    }
    Ok(DownloadedAsset {
        bytes,
        mime_type,
        filename: None,
    })
}

async fn read_response_limited(
    response: Response,
    limit: usize,
    label: &str,
) -> Result<Vec<u8>, ProviderError> {
    if let Some(length) = response.content_length()
        && length > u64::try_from(limit).unwrap_or(u64::MAX)
    {
        return Err(payload_too_large(
            label,
            length,
            u64::try_from(limit).unwrap_or(u64::MAX),
        ));
    }
    let mut bytes = Vec::with_capacity(
        response
            .content_length()
            .and_then(|length| usize::try_from(length).ok())
            .unwrap_or_default()
            .min(limit),
    );
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(payload_too_large(
                label,
                u64::try_from(bytes.len().saturating_add(chunk.len())).unwrap_or(u64::MAX),
                u64::try_from(limit).unwrap_or(u64::MAX),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn payload_too_large(label: &str, actual: u64, limit: u64) -> ProviderError {
    let mut error =
        ProviderError::validation(format!("{label} exceeds the {limit} byte safety limit"));
    error.code = Some("payload_too_large".to_owned());
    error.details = Some(serde_json::json!({ "actualBytes": actual, "limitBytes": limit }));
    error
}

pub(crate) fn encode_path_segment(value: &str) -> Result<String, ProviderError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(ProviderError::validation("remote file ID is required"));
    }
    Ok(url::form_urlencoded::byte_serialize(value.as_bytes()).collect())
}

fn validate_config(
    config: &ProviderConfig,
    credentials: &ProviderCredentials,
) -> Result<(), ProviderError> {
    if config.base_url.trim().is_empty() {
        return Err(ProviderError::new(
            ProviderErrorKind::Configuration,
            "base URL is required",
        ));
    }
    reqwest::Url::parse(&config.base_url).map_err(|error| {
        ProviderError::new(
            ProviderErrorKind::Configuration,
            format!("invalid base URL: {error}"),
        )
    })?;
    if credentials.api_key.trim().is_empty() {
        return Err(ProviderError::new(
            ProviderErrorKind::Configuration,
            "API key is required",
        ));
    }
    for protected in ["authorization", "host", "content-length"] {
        if config
            .headers
            .keys()
            .any(|name| name.eq_ignore_ascii_case(protected))
        {
            return Err(ProviderError::new(
                ProviderErrorKind::Configuration,
                format!("custom header {protected} is protected"),
            ));
        }
    }
    Ok(())
}

fn api_error_message(value: &Value) -> Option<String> {
    value
        .pointer("/error/message")
        .or_else(|| value.get("message"))
        .or_else(|| value.get("detail"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn api_error_code(value: &Value) -> Option<String> {
    value
        .pointer("/error/code")
        .or_else(|| value.pointer("/error/status"))
        .or_else(|| value.get("code"))
        .and_then(|code| match code {
            Value::String(code) => Some(code.clone()),
            Value::Number(code) => Some(code.to_string()),
            _ => None,
        })
}

fn request_id_from_headers(headers: &reqwest::header::HeaderMap) -> Option<String> {
    ["x-request-id", "request-id", "x-goog-request-id"]
        .iter()
        .find_map(|name| {
            headers
                .get(*name)
                .and_then(|value| value.to_str().ok())
                .map(ToOwned::to_owned)
        })
}

fn filename_from_url(url: &str) -> Option<String> {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|url| url.path_segments()?.next_back().map(ToOwned::to_owned))
        .filter(|name| !name.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_local_or_inline_data_as_data_uri() {
        let asset = InputAsset::Base64 {
            data: "YWJj".into(),
            mime_type: "image/png".into(),
            label: None,
        };
        assert_eq!(
            input_to_data_uri(&asset).unwrap(),
            "data:image/png;base64,YWJj"
        );
    }

    #[test]
    fn rejects_protected_custom_headers() {
        let mut config = ProviderConfig::openai("id", "name");
        config
            .headers
            .insert("Authorization".into(), "secret".into());
        let result = HttpTransport::new(
            config,
            ProviderCredentials {
                api_key: "key".into(),
            },
        );
        assert!(result.is_err());
    }
}
