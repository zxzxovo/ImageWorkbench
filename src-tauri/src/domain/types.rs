use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub type ProviderProfileId = String;
pub type ProjectId = String;
pub type RunId = String;
pub type JobId = String;
pub type AssetId = String;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    OpenAi,
    #[serde(rename = "xai")]
    XAi,
    Gemini,
    OpenAiCompatible,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthScheme {
    #[default]
    Bearer,
    Header {
        name: String,
        prefix: Option<String>,
    },
    QueryParameter {
        name: String,
    },
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProfile {
    pub id: ProviderProfileId,
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    pub credential_ref: Option<String>,
    #[serde(default)]
    pub custom_headers: BTreeMap<String, String>,
    #[serde(default)]
    pub secret_header_refs: BTreeMap<String, String>,
    pub timeout_ms: u64,
    pub proxy_url: Option<String>,
    pub organization: Option<String>,
    pub project: Option<String>,
    pub api_version: Option<String>,
    #[serde(default)]
    pub auth_scheme: AuthScheme,
    pub models_path: Option<String>,
    #[serde(default)]
    #[specta(type = std::collections::BTreeMap<String, specta_typescript::Unknown>)]
    pub extra: BTreeMap<String, Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ProviderProfile {
    pub fn new(name: impl Into<String>, kind: ProviderKind, base_url: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            name: name.into(),
            kind,
            base_url: base_url.into(),
            credential_ref: None,
            custom_headers: BTreeMap::new(),
            secret_header_refs: BTreeMap::new(),
            timeout_ms: 120_000,
            proxy_url: None,
            organization: None,
            project: None,
            api_version: None,
            auth_scheme: AuthScheme::Bearer,
            models_path: None,
            extra: BTreeMap::new(),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn official_template(name: impl Into<String>, kind: ProviderKind) -> Self {
        let name = name.into();
        match kind {
            ProviderKind::OpenAi => Self::new(name, kind, "https://api.openai.com/v1"),
            ProviderKind::XAi => Self::new(name, kind, "https://api.x.ai/v1"),
            ProviderKind::Gemini => {
                let mut profile = Self::new(
                    name,
                    kind,
                    "https://generativelanguage.googleapis.com/v1beta",
                );
                profile.auth_scheme = AuthScheme::Header {
                    name: "x-goog-api-key".to_owned(),
                    prefix: None,
                };
                profile.api_version = Some("v1beta".to_owned());
                profile
            }
            ProviderKind::OpenAiCompatible => Self::new(name, kind, "http://localhost:8000/v1"),
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Generate,
    Edit,
    Variation,
    ConversationContinue,
    VideoReferenceToImage,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    Realtime,
    Background,
    ProviderBatch,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum InputAssetKind {
    Image,
    Mask,
    Video,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum OutputModality {
    Image,
    Text,
    Thought,
    Citation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputSource {
    LocalPath { path: PathBuf },
    Url { url: String },
    Base64 { data: String },
    ProviderFile { file_id: String },
    GeneratedOutput { output_part_id: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InputAsset {
    pub id: AssetId,
    pub kind: InputAssetKind,
    pub source: InputSource,
    pub mime_type: Option<String>,
    pub role: Option<String>,
    pub label: Option<String>,
    pub sha256: Option<String>,
    pub size_bytes: Option<u64>,
    #[serde(default)]
    #[specta(type = std::collections::BTreeMap<String, specta_typescript::Unknown>)]
    pub metadata: BTreeMap<String, Value>,
}

impl InputAsset {
    pub fn local(kind: InputAssetKind, path: impl Into<PathBuf>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            kind,
            source: InputSource::LocalPath { path: path.into() },
            mime_type: None,
            role: None,
            label: None,
            sha256: None,
            size_bytes: None,
            metadata: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ImageSize {
    #[default]
    Auto,
    Preset {
        value: String,
    },
    Custom {
        width: u32,
        height: u32,
    },
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum ImageFormat {
    Png,
    Jpeg,
    Webp,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum Background {
    Auto,
    Opaque,
    Transparent,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum ResponseFormat {
    Url,
    Base64Json,
    Inline,
    ProviderFile,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OutputSpec {
    pub count: u16,
    #[serde(default)]
    pub size: ImageSize,
    pub aspect_ratio: Option<String>,
    pub resolution: Option<String>,
    pub quality: Option<String>,
    pub format: Option<ImageFormat>,
    pub compression: Option<u8>,
    pub background: Option<Background>,
    pub style: Option<String>,
    pub moderation: Option<String>,
    pub response_format: Option<ResponseFormat>,
    #[serde(default)]
    pub modalities: BTreeSet<OutputModality>,
    pub remote_storage: Option<bool>,
    pub ttl_seconds: Option<u64>,
}

impl Default for OutputSpec {
    fn default() -> Self {
        Self {
            count: 1,
            size: ImageSize::Auto,
            aspect_ratio: None,
            resolution: None,
            quality: None,
            format: None,
            compression: None,
            background: None,
            style: None,
            moderation: None,
            response_format: None,
            modalities: BTreeSet::from([OutputModality::Image]),
            remote_storage: None,
            ttl_seconds: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GenerationRequest {
    pub id: String,
    pub project_id: ProjectId,
    pub run_group_id: Option<String>,
    pub provider_profile_id: ProviderProfileId,
    pub model_id: String,
    pub operation: Operation,
    pub execution_mode: ExecutionMode,
    pub prompt: String,
    pub final_prompt: Option<String>,
    #[serde(default)]
    pub context_ids: Vec<String>,
    pub preset_id: Option<String>,
    pub conversation_id: Option<String>,
    pub previous_interaction_id: Option<String>,
    #[serde(default)]
    pub inputs: Vec<InputAsset>,
    pub mask: Option<InputAsset>,
    #[serde(default)]
    pub output: OutputSpec,
    #[serde(default)]
    #[specta(type = std::collections::BTreeMap<String, specta_typescript::Unknown>)]
    pub parameters: BTreeMap<String, Value>,
    #[serde(default)]
    #[specta(type = std::collections::BTreeMap<String, specta_typescript::Unknown>)]
    pub metadata: BTreeMap<String, Value>,
    pub created_at: DateTime<Utc>,
}

impl GenerationRequest {
    pub fn new(
        project_id: impl Into<String>,
        provider_profile_id: impl Into<String>,
        model_id: impl Into<String>,
        prompt: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            project_id: project_id.into(),
            run_group_id: None,
            provider_profile_id: provider_profile_id.into(),
            model_id: model_id.into(),
            operation: Operation::Generate,
            execution_mode: ExecutionMode::Realtime,
            prompt: prompt.into(),
            final_prompt: None,
            context_ids: Vec::new(),
            preset_id: None,
            conversation_id: None,
            previous_interaction_id: None,
            inputs: Vec::new(),
            mask: None,
            output: OutputSpec::default(),
            parameters: BTreeMap::new(),
            metadata: BTreeMap::new(),
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SizeCapability {
    pub supports_auto: bool,
    #[serde(default)]
    pub presets: BTreeSet<String>,
    pub allow_custom: bool,
    pub min_width: Option<u32>,
    pub max_width: Option<u32>,
    pub min_height: Option<u32>,
    pub max_height: Option<u32>,
    pub max_area: Option<u64>,
    pub width_multiple_of: Option<u32>,
    pub height_multiple_of: Option<u32>,
    pub min_aspect_ratio: Option<f64>,
    pub max_aspect_ratio: Option<f64>,
}

impl Default for SizeCapability {
    fn default() -> Self {
        Self {
            supports_auto: true,
            presets: BTreeSet::new(),
            allow_custom: false,
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            max_area: None,
            width_multiple_of: None,
            height_multiple_of: None,
            min_aspect_ratio: None,
            max_aspect_ratio: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ParameterValueKind {
    Any,
    Boolean,
    Integer,
    Number,
    String,
    Array,
    Object,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ParameterRule {
    pub value_kind: ParameterValueKind,
    pub required: bool,
    #[serde(default)]
    #[specta(type = Vec<specta_typescript::Unknown>)]
    pub allowed_values: Vec<Value>,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelCapability {
    pub schema_version: u32,
    pub provider_kind: ProviderKind,
    pub model_id: String,
    pub display_name: String,
    #[serde(default)]
    pub aliases: BTreeSet<String>,
    pub documentation_url: Option<String>,
    #[serde(default)]
    pub operations: BTreeSet<Operation>,
    #[serde(default)]
    pub execution_modes: BTreeSet<ExecutionMode>,
    #[serde(default)]
    pub input_kinds: BTreeSet<InputAssetKind>,
    #[serde(default)]
    pub output_modalities: BTreeSet<OutputModality>,
    pub max_prompt_chars: Option<u32>,
    pub max_reference_images: u16,
    pub max_video_inputs: u16,
    pub max_outputs: u16,
    pub max_inline_bytes: Option<u64>,
    pub max_file_bytes: Option<u64>,
    #[serde(default)]
    pub sizes: SizeCapability,
    #[serde(default)]
    pub aspect_ratios: BTreeSet<String>,
    #[serde(default)]
    pub resolutions: BTreeSet<String>,
    #[serde(default)]
    pub qualities: BTreeSet<String>,
    #[serde(default)]
    pub formats: BTreeSet<ImageFormat>,
    #[serde(default)]
    pub response_formats: BTreeSet<ResponseFormat>,
    #[serde(default)]
    pub backgrounds: BTreeSet<Background>,
    #[serde(default)]
    pub styles: BTreeSet<String>,
    #[serde(default)]
    pub moderation_modes: BTreeSet<String>,
    pub supports_mask: bool,
    pub supports_streaming: bool,
    pub supports_partial_images: bool,
    pub supports_interleaved_output: bool,
    pub supports_thinking: bool,
    pub supports_search: bool,
    pub supports_provider_files: bool,
    pub supports_remote_storage: bool,
    pub supports_ttl: bool,
    pub min_ttl_seconds: Option<u64>,
    pub max_ttl_seconds: Option<u64>,
    pub allow_unknown_parameters: bool,
    #[serde(default)]
    pub parameter_rules: BTreeMap<String, ParameterRule>,
    #[serde(default)]
    #[specta(type = std::collections::BTreeMap<String, specta_typescript::Unknown>)]
    pub defaults: BTreeMap<String, Value>,
}

impl ModelCapability {
    pub fn generic(provider_kind: ProviderKind, model_id: impl Into<String>) -> Self {
        let model_id = model_id.into();
        Self {
            schema_version: 1,
            provider_kind,
            display_name: model_id.clone(),
            model_id,
            aliases: BTreeSet::new(),
            documentation_url: None,
            operations: BTreeSet::from([Operation::Generate]),
            execution_modes: BTreeSet::from([ExecutionMode::Realtime]),
            input_kinds: BTreeSet::from([InputAssetKind::Image]),
            output_modalities: BTreeSet::from([OutputModality::Image]),
            max_prompt_chars: None,
            max_reference_images: 0,
            max_video_inputs: 0,
            max_outputs: 1,
            max_inline_bytes: None,
            max_file_bytes: None,
            sizes: SizeCapability::default(),
            aspect_ratios: BTreeSet::new(),
            resolutions: BTreeSet::new(),
            qualities: BTreeSet::new(),
            formats: BTreeSet::new(),
            response_formats: BTreeSet::new(),
            backgrounds: BTreeSet::new(),
            styles: BTreeSet::new(),
            moderation_modes: BTreeSet::new(),
            supports_mask: false,
            supports_streaming: false,
            supports_partial_images: false,
            supports_interleaved_output: false,
            supports_thinking: false,
            supports_search: false,
            supports_provider_files: false,
            supports_remote_storage: false,
            supports_ttl: false,
            min_ttl_seconds: None,
            max_ttl_seconds: None,
            allow_unknown_parameters: false,
            parameter_rules: BTreeMap::new(),
            defaults: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ContextPlacement {
    Prepend,
    Append,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PromptContext {
    pub id: String,
    pub name: String,
    pub content: String,
    pub placement: ContextPlacement,
    pub sort_order: i32,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl PromptContext {
    pub fn new(name: impl Into<String>, content: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            name: name.into(),
            content: content.into(),
            placement: ContextPlacement::Prepend,
            sort_order: 0,
            enabled: true,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GenerationPreset {
    pub id: String,
    pub name: String,
    pub provider_profile_id: Option<String>,
    pub model_id: Option<String>,
    pub operation: Option<Operation>,
    #[serde(default)]
    #[specta(type = std::collections::BTreeMap<String, specta_typescript::Unknown>)]
    pub parameters: BTreeMap<String, Value>,
    pub output: Option<OutputSpec>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Running,
    Paused,
    Succeeded,
    PartiallySucceeded,
    Failed,
    Cancelled,
    Interrupted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Submitting,
    Running,
    WaitingRemote,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RunRecord {
    pub id: RunId,
    pub group_id: Option<String>,
    pub request: GenerationRequest,
    pub status: RunStatus,
    pub raw_prompt: String,
    pub final_prompt: String,
    #[serde(default)]
    pub context_snapshot: Vec<PromptContext>,
    pub preset_snapshot: Option<GenerationPreset>,
    pub capability_snapshot: ModelCapability,
    pub capability_registry_version: String,
    pub model_version: Option<String>,
    pub provider_request_id: Option<String>,
    #[specta(type = Option<specta_typescript::Unknown>)]
    pub redacted_request: Option<Value>,
    #[specta(type = Option<specta_typescript::Unknown>)]
    pub redacted_response: Option<Value>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct JobRecord {
    pub id: JobId,
    pub run_id: RunId,
    pub sequence: u16,
    pub status: JobStatus,
    pub attempt: u16,
    pub remote_job_id: Option<String>,
    pub remote_batch_id: Option<String>,
    pub next_poll_at: Option<DateTime<Utc>>,
    pub request: GenerationRequest,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum OutputPartKind {
    Image,
    Text,
    Thought,
    Citation,
    SearchSuggestion,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OutputPart {
    pub id: String,
    pub run_id: RunId,
    pub job_id: Option<JobId>,
    pub sequence: u32,
    pub kind: OutputPartKind,
    pub text: Option<String>,
    pub local_path: Option<PathBuf>,
    pub remote_url: Option<String>,
    pub provider_file_id: Option<String>,
    pub mime_type: Option<String>,
    pub sha256: Option<String>,
    pub size_bytes: Option<u64>,
    #[serde(default)]
    #[specta(type = std::collections::BTreeMap<String, specta_typescript::Unknown>)]
    pub metadata: BTreeMap<String, Value>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UsageRecord {
    pub id: String,
    pub run_id: RunId,
    pub job_id: Option<JobId>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub image_count: Option<u64>,
    pub input_bytes: Option<u64>,
    pub output_bytes: Option<u64>,
    pub cost_micros: Option<u64>,
    pub currency: Option<String>,
    #[serde(default)]
    #[specta(type = std::collections::BTreeMap<String, specta_typescript::Unknown>)]
    pub details: BTreeMap<String, Value>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProviderError {
    pub code: String,
    pub message: String,
    pub http_status: Option<u16>,
    pub retryable: bool,
    pub request_id: Option<String>,
    pub provider: Option<ProviderKind>,
    #[specta(type = Option<specta_typescript::Unknown>)]
    pub details: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ErrorRecord {
    pub id: String,
    pub run_id: RunId,
    pub job_id: Option<JobId>,
    pub error: ProviderError,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum RunEvent {
    StatusChanged { run_id: RunId, status: RunStatus },
    JobChanged { job: Box<JobRecord> },
    PartialOutput { output: OutputPart },
    OutputStored { output: OutputPart },
    UsageUpdated { usage: UsageRecord },
    Error { error: ErrorRecord },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub id: ProjectId,
    pub name: String,
    pub root_path: PathBuf,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_opened_at: DateTime<Utc>,
    pub default_provider_profile_id: Option<String>,
    pub default_model_id: Option<String>,
    #[serde(default)]
    #[specta(type = std::collections::BTreeMap<String, specta_typescript::Unknown>)]
    pub default_parameters: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DomainError {
    Serialization(String),
    InvalidCapability(String),
    Validation(Vec<crate::domain::ValidationIssue>),
}

impl fmt::Display for DomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Serialization(message) => write!(f, "serialization failed: {message}"),
            Self::InvalidCapability(message) => write!(f, "invalid capability: {message}"),
            Self::Validation(issues) => write!(
                f,
                "request validation failed with {} issue(s)",
                issues.len()
            ),
        }
    }
}

impl std::error::Error for DomainError {}
