use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::domain::{
    AuthScheme as DomainAuthScheme, ErrorRecord, GenerationPreset, OutputPart, ProjectSummary,
    PromptContext, ProviderKind as DomainProviderKind, ProviderProfile as DomainProviderProfile,
    RunRecord, UsageRecord,
};
use crate::providers::error::{ProviderError, ProviderErrorKind};
use crate::providers::types::{
    AuthScheme, ProviderConfig, ProviderCredentials, ProviderKind as AdapterProviderKind,
};
use crate::security::CredentialKey;
use crate::storage::StorageError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub enum Locale {
    #[serde(rename = "zh-CN")]
    ZhCn,
    #[serde(rename = "en-US")]
    EnUs,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum FrontendProviderKind {
    Openai,
    Xai,
    Gemini,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum ApiMode {
    Native,
    OpenaiCompatible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum FrontendAuthScheme {
    Bearer,
    Header,
    Query,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProviderHeaderDto {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub value: String,
    pub secret: bool,
    pub has_stored_value: Option<bool>,
}

impl ProviderHeaderDto {
    pub fn credential_key(&self, provider_id: &str) -> CredentialKey {
        CredentialKey {
            service: "dev.imageworkbench.desktop".to_owned(),
            account: format!("provider-header:{provider_id}:{}", self.id),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProfileDto {
    pub id: String,
    pub name: String,
    pub kind: FrontendProviderKind,
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    pub has_stored_secret: Option<bool>,
    pub api_mode: ApiMode,
    pub enabled: bool,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub discovered_models: Vec<String>,
    pub api_version: Option<String>,
    pub organization: Option<String>,
    pub project_id: Option<String>,
    pub custom_header: Option<String>,
    pub last_synced_at: Option<String>,
    pub timeout_seconds: Option<u64>,
    pub timeout_ms: Option<u64>,
    pub proxy_url: Option<String>,
    pub auth_scheme: Option<FrontendAuthScheme>,
    pub auth_header_name: Option<String>,
    pub auth_prefix: Option<String>,
    pub auth_query_name: Option<String>,
    #[serde(default)]
    pub custom_headers: Vec<ProviderHeaderDto>,
    pub models_path: Option<String>,
    #[serde(alias = "extraJson")]
    pub compatibility_json: Option<String>,
    pub capability_overrides_json: Option<String>,
    /// Default streaming preference: None = follow model capability, Some(true/false) = explicit override.
    #[serde(default)]
    pub default_stream: Option<bool>,
}

impl ProviderProfileDto {
    pub fn adapter_kind(&self) -> AdapterProviderKind {
        if self.api_mode == ApiMode::OpenaiCompatible || self.kind == FrontendProviderKind::Custom {
            return AdapterProviderKind::OpenAiCompatible;
        }
        match self.kind {
            FrontendProviderKind::Openai => AdapterProviderKind::OpenAi,
            FrontendProviderKind::Xai => AdapterProviderKind::Xai,
            FrontendProviderKind::Gemini => AdapterProviderKind::Gemini,
            FrontendProviderKind::Custom => AdapterProviderKind::OpenAiCompatible,
        }
    }

    pub fn credential_key(&self) -> CredentialKey {
        CredentialKey::provider(&self.id)
    }

    pub fn to_provider_config(&self) -> CommandResult<ProviderConfig> {
        if self.id.trim().is_empty() {
            return Err(CommandError::validation("provider ID is required"));
        }
        if self.name.trim().is_empty() {
            return Err(CommandError::validation("provider name is required"));
        }
        let raw_base_url = self.base_url.trim();
        if raw_base_url.is_empty() {
            return Err(CommandError::validation("provider base URL is required"));
        }
        let kind = self.adapter_kind();
        let base_url = normalize_base_url(raw_base_url, kind, self.api_version.as_deref())?;
        let auth = match self.auth_scheme {
            Some(FrontendAuthScheme::Bearer) => AuthScheme::Bearer,
            Some(FrontendAuthScheme::Header) => AuthScheme::Header {
                name: required_header_name(
                    self.auth_header_name
                        .as_deref()
                        .or(self.custom_header.as_deref()),
                )?,
                prefix: self.auth_prefix.clone(),
            },
            Some(FrontendAuthScheme::Query) => AuthScheme::Query {
                name: self
                    .auth_query_name
                    .as_deref()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .ok_or_else(|| CommandError::validation("queryName is required"))?
                    .to_owned(),
            },
            None => {
                if let Some(header) = self
                    .custom_header
                    .as_deref()
                    .map(str::trim)
                    .filter(|header| !header.is_empty())
                {
                    AuthScheme::Header {
                        name: required_header_name(Some(header))?,
                        prefix: None,
                    }
                } else if kind == AdapterProviderKind::Gemini {
                    AuthScheme::Header {
                        name: "x-goog-api-key".to_owned(),
                        prefix: None,
                    }
                } else {
                    AuthScheme::Bearer
                }
            }
        };
        let protected_auth_header = match &auth {
            AuthScheme::Header { name, .. } => Some(name.as_str()),
            _ => None,
        };
        let headers = validate_custom_headers(&self.custom_headers, protected_auth_header)?;
        let timeout_secs = self
            .timeout_ms
            .map(|milliseconds| milliseconds.saturating_add(999) / 1_000)
            .or(self.timeout_seconds)
            .unwrap_or(300)
            .clamp(1, 3_600);
        if let Some(proxy_url) = self
            .proxy_url
            .as_deref()
            .filter(|url| !url.trim().is_empty())
        {
            url::Url::parse(proxy_url)
                .map_err(|error| CommandError::validation(format!("invalid proxy URL: {error}")))?;
        }
        self.parsed_extra_json()?;
        self.parsed_capability_overrides()?;
        Ok(ProviderConfig {
            id: self.id.clone(),
            name: self.name.clone(),
            kind,
            base_url,
            auth,
            headers,
            timeout_secs,
            proxy_url: self.proxy_url.clone(),
            organization: self.organization.clone(),
            project: self.project_id.clone(),
            api_version: if kind == AdapterProviderKind::Gemini {
                Some(
                    self.api_version
                        .clone()
                        .unwrap_or_else(|| "v1beta".to_owned()),
                )
            } else {
                self.api_version.clone()
            },
            models_path: self.models_path.clone(),
        })
    }

    pub fn to_domain_profile(&self) -> CommandResult<DomainProviderProfile> {
        let config = self.to_provider_config()?;
        let kind = match config.kind {
            AdapterProviderKind::OpenAi => DomainProviderKind::OpenAi,
            AdapterProviderKind::Xai => DomainProviderKind::XAi,
            AdapterProviderKind::Gemini => DomainProviderKind::Gemini,
            AdapterProviderKind::OpenAiCompatible => DomainProviderKind::OpenAiCompatible,
        };
        let auth_scheme = match config.auth {
            AuthScheme::Bearer => DomainAuthScheme::Bearer,
            AuthScheme::Header { name, prefix } => DomainAuthScheme::Header { name, prefix },
            AuthScheme::Query { name } => DomainAuthScheme::QueryParameter { name },
        };
        let now = Utc::now();
        let mut extra = BTreeMap::from([
            ("enabled".to_owned(), Value::Bool(self.enabled)),
            ("models".to_owned(), serde_json::json!(self.models)),
            (
                "discoveredModels".to_owned(),
                serde_json::json!(self.discovered_models),
            ),
        ]);
        if let Some(connection_extra) = self.parsed_extra_json()? {
            extra.insert(
                "connectionExtra".to_owned(),
                crate::security::redact_json(&Value::Object(connection_extra)),
            );
        }
        let capability_overrides = self.parsed_capability_overrides()?;
        if !capability_overrides.is_empty() {
            extra.insert(
                "capabilityOverrides".to_owned(),
                Value::Object(capability_overrides),
            );
        }
        Ok(DomainProviderProfile {
            id: self.id.clone(),
            name: self.name.clone(),
            kind,
            base_url: config.base_url.clone(),
            credential_ref: Some(self.credential_key().reference()),
            custom_headers: self
                .custom_headers
                .iter()
                .filter(|header| !header.secret && !header.value.is_empty())
                .map(|header| (header.name.clone(), header.value.clone()))
                .collect(),
            secret_header_refs: self
                .custom_headers
                .iter()
                .filter(|header| header.secret)
                .map(|header| {
                    (
                        header.name.clone(),
                        header.credential_key(&self.id).reference(),
                    )
                })
                .collect(),
            timeout_ms: config.timeout_secs.saturating_mul(1_000),
            proxy_url: config.proxy_url,
            organization: self.organization.clone(),
            project: self.project_id.clone(),
            api_version: self.api_version.clone(),
            auth_scheme,
            models_path: config.models_path,
            extra,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn credentials(&self, api_key: String) -> ProviderCredentials {
        ProviderCredentials { api_key }
    }

    fn parsed_extra_json(&self) -> CommandResult<Option<Map<String, Value>>> {
        let Some(source) = self
            .compatibility_json
            .as_deref()
            .map(str::trim)
            .filter(|source| !source.is_empty())
        else {
            return Ok(None);
        };
        let value: Value = serde_json::from_str(source).map_err(|error| {
            CommandError::validation(format!("invalid provider extraJson: {error}"))
        })?;
        value
            .as_object()
            .cloned()
            .map(Some)
            .ok_or_else(|| CommandError::validation("provider extraJson must be an object"))
    }

    pub(crate) fn parsed_capability_overrides(&self) -> CommandResult<Map<String, Value>> {
        let Some(source) = self
            .capability_overrides_json
            .as_deref()
            .map(str::trim)
            .filter(|source| !source.is_empty())
        else {
            return Ok(Map::new());
        };
        let value: Value = serde_json::from_str(source).map_err(|error| {
            CommandError::validation(format!("invalid capabilityOverridesJson: {error}"))
        })?;
        let overrides = value
            .as_object()
            .cloned()
            .ok_or_else(|| CommandError::validation("capabilityOverridesJson must be an object"))?;
        for (model, patch) in &overrides {
            if model.trim().is_empty() {
                return Err(CommandError::validation(
                    "capability override model IDs cannot be empty",
                ));
            }
            if !patch.is_object() {
                return Err(CommandError::validation(format!(
                    "capability override for {model} must be an object"
                )));
            }
            crate::providers::capabilities::resolve_model(self.adapter_kind(), model, Some(patch))?;
        }
        Ok(overrides)
    }
}

fn required_header_name(value: Option<&str>) -> CommandResult<String> {
    let value = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CommandError::validation("authHeaderName is required"))?;
    reqwest::header::HeaderName::from_bytes(value.as_bytes())
        .map_err(|error| CommandError::validation(format!("invalid auth header name: {error}")))?;
    Ok(value.to_owned())
}

fn normalize_base_url(
    value: &str,
    kind: AdapterProviderKind,
    api_version: Option<&str>,
) -> CommandResult<String> {
    let mut url = url::Url::parse(value)
        .map_err(|error| CommandError::validation(format!("invalid base URL: {error}")))?;
    if kind == AdapterProviderKind::Gemini {
        let version = api_version
            .map(str::trim)
            .filter(|version| !version.is_empty())
            .unwrap_or("v1beta")
            .trim_matches('/');
        let mut segments = url
            .path_segments()
            .map(|segments| {
                segments
                    .filter(|segment| !segment.is_empty())
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if segments
            .last()
            .is_some_and(|segment| matches!(segment.as_str(), "v1" | "v1beta"))
        {
            segments.pop();
        }
        segments.push(version.to_owned());
        url.set_path(&format!("/{}", segments.join("/")));
    }
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

fn validate_custom_headers(
    headers: &[ProviderHeaderDto],
    auth_header: Option<&str>,
) -> CommandResult<BTreeMap<String, String>> {
    const PROTECTED: &[&str] = &[
        "authorization",
        "proxy-authorization",
        "x-goog-api-key",
        "host",
        "content-length",
    ];
    let mut result = BTreeMap::new();
    for header in headers {
        let name = header.name.trim();
        reqwest::header::HeaderName::from_bytes(name.as_bytes()).map_err(|error| {
            CommandError::validation(format!("invalid custom header name {name}: {error}"))
        })?;
        if !header.value.is_empty() {
            reqwest::header::HeaderValue::from_str(&header.value).map_err(|error| {
                CommandError::validation(format!("invalid custom header value for {name}: {error}"))
            })?;
        }
        if PROTECTED
            .iter()
            .any(|protected| name.eq_ignore_ascii_case(protected))
            || auth_header.is_some_and(|auth| name.eq_ignore_ascii_case(auth))
        {
            return Err(CommandError::validation(format!(
                "custom headers cannot override protected header {name}"
            )));
        }
        if !header.value.is_empty() {
            result.insert(name.to_owned(), header.value.clone());
        }
    }
    Ok(result)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum WorkspaceTab {
    Create,
    History,
    Descriptions,
    Presets,
    ProjectSettings,
    Results,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum GenerationMode {
    Generate,
    Edit,
    Mask,
    Video,
    Variation,
    ConversationContinue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum FrontendTaskStatus {
    Queued,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum DescriptionPlacement {
    Prefix,
    Suffix,
}

impl Default for DescriptionPlacement {
    fn default() -> Self {
        Self::Prefix
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CommonDescriptionDto {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub content: String,
    pub enabled: bool,
    #[serde(default)]
    pub placement: DescriptionPlacement,
    #[serde(default)]
    pub prefix_content: String,
    #[serde(default)]
    pub suffix_content: String,
    #[serde(default)]
    pub negative_content: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FrontendGenerationPreset {
    pub id: String,
    pub name: String,
    pub description: String,
    pub provider_id: String,
    pub model: String,
    pub mode: GenerationMode,
    pub aspect_ratio: String,
    pub size: String,
    pub quality: String,
    pub output_format: String,
    pub response_format: Option<String>,
    pub prompt_template: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSettingsDto {
    pub use_common_descriptions: bool,
    pub save_metadata: bool,
    pub save_raw_response: bool,
    pub auto_open_folder: bool,
    pub naming_pattern: String,
    pub default_provider_id: String,
    pub default_model: String,
    #[serde(default)]
    pub flat_output: bool,
    #[serde(default)]
    pub default_stream: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDto {
    pub id: String,
    pub name: String,
    pub description: String,
    pub storage_path: String,
    pub created_at: String,
    pub updated_at: String,
    pub color: String,
    #[serde(default)]
    pub descriptions: Vec<CommonDescriptionDto>,
    #[serde(default)]
    pub presets: Vec<FrontendGenerationPreset>,
    pub settings: ProjectSettingsDto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ProjectHealthStatus {
    Opened,
    Missing,
    Locked,
    Corrupt,
    IdMismatch,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRecoveryDto {
    pub project_id: String,
    pub storage_path: String,
    pub status: ProjectHealthStatus,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ProjectCopyMode {
    Full,
    Configuration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDeleteResult {
    pub removed: bool,
    pub files_deleted: bool,
    pub file_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMoveResult {
    pub project: ProjectSummary,
    pub original_files_deleted: bool,
    pub cleanup_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum ReferenceRole {
    Object,
    Character,
    Style,
    Source,
    Video,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum ReferenceSourceType {
    Local,
    Url,
    Base64,
    FileId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceAssetDto {
    pub id: String,
    pub name: String,
    pub url: String,
    pub mime_type: String,
    pub role: ReferenceRole,
    pub source_type: Option<ReferenceSourceType>,
    pub file_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GenerationDraftDto {
    pub provider_id: String,
    pub model: String,
    pub mode: GenerationMode,
    pub prompt: String,
    pub negative_prompt: String,
    pub aspect_ratio: String,
    pub size: String,
    pub quality: String,
    pub count: u16,
    pub output_format: String,
    pub response_format: Option<String>,
    pub background: String,
    pub compression: u8,
    #[serde(default)]
    pub references: Vec<ReferenceAssetDto>,
    pub mask_data_url: Option<String>,
    pub seed: String,
    pub input_fidelity: String,
    pub thinking_level: String,
    pub web_search: bool,
    pub image_search: bool,
    pub include_text: bool,
    pub stream: bool,
    pub background_task: bool,
    pub batch: bool,
    pub partial_images: u8,
    pub service_tier: String,
    pub temperature: f64,
    pub top_p: f64,
    pub store_interaction: bool,
    pub custom_json: String,
    pub previous_response_id: Option<String>,
    pub previous_interaction_id: Option<String>,
    pub use_responses_api: Option<bool>,
    pub use_interactions_api: Option<bool>,
    pub response_model: Option<String>,
    pub image_generation_action: Option<String>,
    pub moderation: Option<String>,
    pub style: Option<String>,
    pub user: Option<String>,
    pub xai_storage_filename: Option<String>,
    pub xai_expires_after: Option<u32>,
    pub xai_public_url: Option<bool>,
    pub xai_public_url_expires_after: Option<u32>,
    pub resume_interaction_id: Option<String>,
    pub last_event_id: Option<String>,
    /// Local output filename override (leave empty to use default naming).
    #[serde(default)]
    pub output_filename: String,
    /// When true, store all outputs flat in the project output directory
    /// instead of per-run subdirectories.
    #[serde(default)]
    pub flat_output: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedAsset {
    pub id: String,
    pub task_id: String,
    pub url: String,
    pub file_path: String,
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub prompt: String,
    pub created_at: String,
    pub selected: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponsePartDto {
    Image {
        id: String,
        asset_id: String,
        url: String,
        mime_type: String,
        width: u32,
        height: u32,
        file_path: String,
    },
    Text {
        id: String,
        text: String,
    },
    Thought {
        id: String,
        summary: String,
        image_url: Option<String>,
    },
    Citation {
        id: String,
        title: Option<String>,
        url: Option<String>,
        snippet: Option<String>,
        start_index: Option<u32>,
        end_index: Option<u32>,
    },
    SearchSuggestions {
        id: String,
        html: String,
    },
    RemoteFile {
        id: String,
        name: String,
        uri: String,
        mime_type: String,
        size_bytes: Option<u64>,
    },
    RemoteJob {
        id: String,
        job_id: String,
        kind: String,
        status: String,
        provider: String,
        model: Option<String>,
    },
    Usage {
        id: String,
        usage: UsageDto,
    },
    RequestMeta {
        id: String,
        request_id: String,
        interaction_id: Option<String>,
        provider_response_id: Option<String>,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UsageDto {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub thought_tokens: u64,
    pub cached_tokens: u64,
    pub total_tokens: u64,
    pub generated_images: u64,
    pub image_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GenerationCommandResult {
    pub run_id: String,
    pub request_id: String,
    pub interaction_id: Option<String>,
    pub failure_reason: Option<String>,
    #[serde(default)]
    pub assets: Vec<GeneratedAsset>,
    #[serde(default)]
    pub response_parts: Vec<ResponsePartDto>,
    #[serde(default)]
    pub usage: UsageDto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GenerationTaskDto {
    pub id: String,
    pub project_id: String,
    pub provider_id: String,
    pub provider_name: String,
    pub model: String,
    pub prompt: String,
    pub composed_prompt: String,
    pub mode: GenerationMode,
    pub status: FrontendTaskStatus,
    pub progress: f64,
    pub count: u16,
    pub created_at: String,
    pub duration_ms: Option<u64>,
    pub error: Option<String>,
    #[serde(default)]
    pub assets: Vec<GeneratedAsset>,
    #[serde(default)]
    pub response_parts: Vec<ResponsePartDto>,
    pub request_id: Option<String>,
    pub interaction_id: Option<String>,
    pub usage: Option<UsageDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRecordDto {
    #[serde(flatten)]
    pub task: GenerationTaskDto,
    pub favorite: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GenerateImagesRequest {
    pub client_task_id: Option<String>,
    pub project_id: String,
    pub storage_path: String,
    pub provider: ProviderProfileDto,
    pub draft: GenerationDraftDto,
    #[serde(default)]
    pub manual_negative_prompt: Option<String>,
    pub composed_prompt: String,
    #[serde(default)]
    pub context_ids: Vec<String>,
    pub preset_id: Option<String>,
    #[serde(default)]
    pub context_snapshot: Vec<PromptContext>,
    pub preset_snapshot: Option<GenerationPreset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportedInputDto {
    pub relative_path: String,
    pub mime_type: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HistoryOutputDto {
    pub output: OutputPart,
    pub preview_data_url: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HistoryDetailsDto {
    pub run: RunRecord,
    pub outputs: Vec<HistoryOutputDto>,
    pub usage: Vec<UsageRecord>,
    pub errors: Vec<ErrorRecord>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDetailsDto {
    pub summary: ProjectSummary,
    pub contexts: Vec<PromptContext>,
    pub presets: Vec<GenerationPreset>,
    pub recent_records: Vec<HistoryDetailsDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GenerationEventEnvelope {
    pub run_id: String,
    #[specta(type = specta_typescript::Unknown)]
    pub event: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    pub locale: Locale,
    #[serde(default)]
    pub theme: ThemeMode,
    pub active_project_id: String,
    #[serde(default)]
    pub projects: Vec<ProjectDto>,
    #[serde(default)]
    pub providers: Vec<ProviderProfileDto>,
    #[serde(default)]
    pub history: Vec<HistoryRecordDto>,
    #[serde(default)]
    pub project_recovery: Vec<ProjectRecoveryDto>,
}

impl WorkspaceSnapshot {
    pub fn without_api_keys(&self) -> Self {
        let mut snapshot = self.clone();
        for provider in &mut snapshot.providers {
            provider.api_key.clear();
            for header in &mut provider.custom_headers {
                if header.secret || is_sensitive_header_name(&header.name) {
                    header.secret = true;
                    header.value.clear();
                }
            }
            if let Some(source) = provider.compatibility_json.as_deref() {
                if let Ok(value) = serde_json::from_str::<Value>(source) {
                    provider.compatibility_json =
                        Some(crate::security::redact_json(&value).to_string());
                }
            }
        }
        snapshot
    }

    pub fn for_global_storage(&self) -> Self {
        let mut snapshot = self.without_api_keys();
        snapshot.history.clear();
        snapshot.project_recovery.clear();
        for project in &mut snapshot.projects {
            project.descriptions.clear();
            project.presets.clear();
        }
        snapshot
    }
}

fn is_sensitive_header_name(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_lowercase().as_str(),
        "authorization" | "proxy-authorization" | "x-api-key" | "api-key" | "x-goog-api-key"
    )
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: String,
    pub message: String,
    #[specta(type = Option<specta_typescript::Unknown>)]
    pub details: Option<Value>,
}

pub type CommandResult<T> = Result<T, CommandError>;

impl CommandError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::new("validation", message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new("not_found", message)
    }

    pub fn cancelled() -> Self {
        Self::new("cancelled", "generation was cancelled")
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for CommandError {}

impl From<StorageError> for CommandError {
    fn from(error: StorageError) -> Self {
        Self::new("storage", error.to_string())
    }
}

impl From<ProviderError> for CommandError {
    fn from(error: ProviderError) -> Self {
        let code = match error.kind {
            ProviderErrorKind::Configuration => "provider_configuration",
            ProviderErrorKind::Validation => "provider_validation",
            ProviderErrorKind::Authentication => "provider_authentication",
            ProviderErrorKind::Permission => "provider_permission",
            ProviderErrorKind::RateLimit => "provider_rate_limit",
            ProviderErrorKind::Http => "provider_http",
            ProviderErrorKind::Api => "provider_api",
            ProviderErrorKind::Parse => "provider_parse",
            ProviderErrorKind::Unsupported => "provider_unsupported",
            ProviderErrorKind::Io => "provider_io",
            ProviderErrorKind::Cancelled => "cancelled",
        };
        Self {
            code: code.to_owned(),
            message: error.message,
            details: error.details,
        }
    }
}

pub fn parse_custom_options(source: &str) -> CommandResult<Map<String, Value>> {
    if source.trim().is_empty() {
        return Ok(Map::new());
    }
    let value: Value = serde_json::from_str(source)
        .map_err(|error| CommandError::validation(format!("invalid custom JSON: {error}")))?;
    value
        .as_object()
        .cloned()
        .ok_or_else(|| CommandError::validation("custom JSON must be an object"))
}

pub fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

pub fn parse_timestamp(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

pub fn string_set(values: impl IntoIterator<Item = String>) -> BTreeSet<String> {
    values.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{ProviderProfileDto, WorkspaceSnapshot};

    #[test]
    fn workspace_persistence_strips_all_secret_values() {
        let snapshot: WorkspaceSnapshot = serde_json::from_value(json!({
            "locale": "zh-CN",
            "activeProjectId": "",
            "projects": [],
            "history": [],
            "providers": [{
                "id": "provider",
                "name": "Custom",
                "kind": "custom",
                "baseUrl": "https://example.com/v1",
                "apiKey": "top-secret",
                "apiMode": "openai-compatible",
                "enabled": true,
                "customHeaders": [{
                    "id": "header",
                    "name": "X-API-Key",
                    "value": "header-secret",
                    "secret": false
                }],
                "compatibilityJson": "{\"api_key\":\"json-secret\",\"mode\":\"images\"}",
                "capabilityOverridesJson": "{\"custom-image\":{\"output_count\":{\"min\":1,\"max\":2}}}"
            }]
        }))
        .unwrap();

        let sanitized = snapshot.without_api_keys();
        let provider = &sanitized.providers[0];
        assert!(provider.api_key.is_empty());
        assert!(provider.custom_headers[0].secret);
        assert!(provider.custom_headers[0].value.is_empty());
        assert!(
            !provider
                .compatibility_json
                .as_deref()
                .unwrap()
                .contains("json-secret")
        );
        assert_eq!(
            provider.capability_overrides_json.as_deref(),
            Some("{\"custom-image\":{\"output_count\":{\"min\":1,\"max\":2}}}")
        );
        assert!(
            provider
                .to_domain_profile()
                .unwrap()
                .extra
                .contains_key("capabilityOverrides")
        );
    }

    #[test]
    fn capability_override_json_and_patches_are_validated() {
        fn provider(source: &str) -> ProviderProfileDto {
            serde_json::from_value(json!({
                "id": "provider",
                "name": "OpenAI",
                "kind": "openai",
                "baseUrl": "https://api.openai.com/v1",
                "apiMode": "native",
                "enabled": true,
                "capabilityOverridesJson": source
            }))
            .unwrap()
        }

        assert!(provider("{").to_provider_config().is_err());
        assert!(provider("[]").to_provider_config().is_err());
        assert!(
            provider(r#"{"gpt-image-1":1}"#)
                .to_provider_config()
                .is_err()
        );
        assert!(
            provider(r#"{"gpt-image-1":{"output_count":"bad"}}"#)
                .to_provider_config()
                .is_err()
        );
        assert!(
            provider(r#"{"future-image":{"operations":["generate"]}}"#)
                .to_provider_config()
                .is_ok()
        );
    }
}
