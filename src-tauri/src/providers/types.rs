use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    OpenAi,
    Xai,
    Gemini,
    OpenAiCompatible,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthScheme {
    Bearer,
    Header {
        name: String,
        prefix: Option<String>,
    },
    Query {
        name: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    pub auth: AuthScheme,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    pub proxy_url: Option<String>,
    pub organization: Option<String>,
    pub project: Option<String>,
    pub api_version: Option<String>,
    pub models_path: Option<String>,
}

const fn default_timeout_secs() -> u64 {
    300
}

impl ProviderConfig {
    pub fn openai(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind: ProviderKind::OpenAi,
            base_url: "https://api.openai.com/v1".into(),
            auth: AuthScheme::Bearer,
            headers: BTreeMap::new(),
            timeout_secs: default_timeout_secs(),
            proxy_url: None,
            organization: None,
            project: None,
            api_version: None,
            models_path: None,
        }
    }

    pub fn xai(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind: ProviderKind::Xai,
            base_url: "https://api.x.ai/v1".into(),
            auth: AuthScheme::Bearer,
            headers: BTreeMap::new(),
            timeout_secs: default_timeout_secs(),
            proxy_url: None,
            organization: None,
            project: None,
            api_version: None,
            models_path: None,
        }
    }

    pub fn gemini(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind: ProviderKind::Gemini,
            base_url: "https://generativelanguage.googleapis.com/v1beta".into(),
            auth: AuthScheme::Header {
                name: "x-goog-api-key".into(),
                prefix: None,
            },
            headers: BTreeMap::new(),
            timeout_secs: default_timeout_secs(),
            proxy_url: None,
            organization: None,
            project: None,
            api_version: Some("v1beta".into()),
            models_path: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProviderCredentials {
    pub api_key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Generate,
    Edit,
    Variation,
    ConversationContinue,
    VideoReferenceToImage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    Realtime,
    Background,
    ProviderBatch,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputAsset {
    Url {
        url: String,
        mime_type: Option<String>,
        label: Option<String>,
    },
    Base64 {
        data: String,
        mime_type: String,
        label: Option<String>,
    },
    FileId {
        id: String,
        mime_type: Option<String>,
        label: Option<String>,
    },
    LocalFile {
        path: PathBuf,
        mime_type: String,
        label: Option<String>,
    },
}

impl InputAsset {
    pub fn mime_type(&self) -> Option<&str> {
        match self {
            Self::Url { mime_type, .. } | Self::FileId { mime_type, .. } => mime_type.as_deref(),
            Self::Base64 { mime_type, .. } | Self::LocalFile { mime_type, .. } => Some(mime_type),
        }
    }

    pub fn is_video(&self) -> bool {
        self.mime_type()
            .is_some_and(|mime| mime.starts_with("video/"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct OutputSpec {
    #[serde(default = "default_output_count")]
    pub count: u8,
    pub size: Option<String>,
    pub aspect_ratio: Option<String>,
    pub resolution: Option<String>,
    pub quality: Option<String>,
    pub format: Option<String>,
    pub response_format: Option<String>,
    pub background: Option<String>,
    pub compression: Option<u8>,
}

const fn default_output_count() -> u8 {
    1
}

impl Default for OutputSpec {
    fn default() -> Self {
        Self {
            count: default_output_count(),
            size: None,
            aspect_ratio: None,
            resolution: None,
            quality: None,
            format: None,
            response_format: None,
            background: None,
            compression: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct OpenAiOptions {
    #[serde(default)]
    pub api_surface: OpenAiApiSurface,
    pub moderation: Option<String>,
    pub style: Option<String>,
    pub user: Option<String>,
    pub input_fidelity: Option<String>,
    #[serde(default)]
    pub stream: bool,
    pub partial_images: Option<u8>,
    pub image_model: Option<String>,
    pub image_generation_action: Option<String>,
    pub previous_response_id: Option<String>,
    #[serde(default)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum OpenAiApiSurface {
    #[default]
    Images,
    Responses,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct XaiOptions {
    pub storage: Option<XaiStorageOptions>,
    #[serde(default)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct XaiStorageOptions {
    pub filename: String,
    pub expires_after: Option<u32>,
    pub public_url: Option<XaiPublicUrlOptions>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(untagged)]
pub enum XaiPublicUrlOptions {
    Enabled(bool),
    Config { expires_after: Option<u32> },
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct GeminiOptions {
    #[serde(default)]
    pub api_surface: GeminiApiSurface,
    #[serde(default = "default_response_modalities")]
    pub response_modalities: Vec<String>,
    pub thinking_level: Option<String>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    #[serde(default)]
    pub include_thoughts: bool,
    #[serde(default)]
    pub google_search: bool,
    #[serde(default)]
    pub image_search: bool,
    #[serde(default)]
    pub stream: bool,
    pub previous_interaction_id: Option<String>,
    pub resume_interaction_id: Option<String>,
    pub last_event_id: Option<String>,
    #[serde(default)]
    pub extra: Map<String, Value>,
}

fn default_response_modalities() -> Vec<String> {
    vec!["IMAGE".into()]
}

impl Default for GeminiOptions {
    fn default() -> Self {
        Self {
            api_surface: GeminiApiSurface::Interactions,
            response_modalities: default_response_modalities(),
            thinking_level: None,
            temperature: None,
            top_p: None,
            include_thoughts: false,
            google_search: false,
            image_search: false,
            stream: false,
            previous_interaction_id: None,
            resume_interaction_id: None,
            last_event_id: None,
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum GeminiApiSurface {
    GenerateContent,
    #[default]
    Interactions,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct ProviderOptions {
    pub openai: Option<OpenAiOptions>,
    pub xai: Option<XaiOptions>,
    pub gemini: Option<GeminiOptions>,
    #[serde(default)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct GenerationRequest {
    pub request_id: String,
    pub model: String,
    #[serde(skip)]
    pub resolved_capability: Option<Box<super::capabilities::ModelCapability>>,
    pub operation: Operation,
    pub execution: ExecutionMode,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub inputs: Vec<InputAsset>,
    pub mask: Option<InputAsset>,
    #[serde(default)]
    pub output: OutputSpec,
    #[serde(default)]
    pub options: ProviderOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct DiscoveredModel {
    pub id: String,
    pub display_name: Option<String>,
    pub owned_by: Option<String>,
    #[serde(default)]
    pub supported_actions: Vec<String>,
    pub raw: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ConnectionTest {
    pub provider: ProviderKind,
    pub latency_ms: u128,
    pub model_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AssetSource {
    Url { url: String },
    Base64 { data: String },
    FileId { id: String },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct RemoteFile {
    pub id: String,
    pub filename: Option<String>,
    pub expires_at: Option<Value>,
    pub public_url: Option<String>,
    pub public_url_expires_at: Option<Value>,
    pub public_url_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutputPart {
    Image {
        source: AssetSource,
        mime_type: Option<String>,
        revised_prompt: Option<String>,
        remote_file: Option<RemoteFile>,
    },
    Text {
        text: String,
        #[serde(default)]
        annotations: Vec<Value>,
    },
    Thought {
        text: String,
        signature: Option<String>,
    },
    Citation {
        title: Option<String>,
        url: Option<String>,
        snippet: Option<String>,
        raw: Value,
    },
    SearchSuggestions {
        html: String,
        signature: Option<String>,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
pub struct UsageRecord {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub image_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    pub raw: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RemoteJobKind {
    Background,
    Batch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RemoteJobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Expired,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct RemoteJob {
    pub id: String,
    pub kind: RemoteJobKind,
    pub status: RemoteJobStatus,
    pub provider: ProviderKind,
    pub model: Option<String>,
    pub raw: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct GenerationResponse {
    pub provider: ProviderKind,
    pub model: Option<String>,
    pub created_at: Option<i64>,
    #[serde(default)]
    pub outputs: Vec<OutputPart>,
    pub usage: Option<UsageRecord>,
    pub remote_job: Option<RemoteJob>,
    pub request_id: Option<String>,
    pub raw: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct BatchItem {
    pub key: String,
    pub endpoint: String,
    pub body: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct BatchSubmission {
    pub name: String,
    pub model: Option<String>,
    #[serde(default)]
    pub requests: Vec<BatchItem>,
    pub input_file_id: Option<String>,
    pub completion_window: Option<String>,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct JobPollResult {
    pub job: RemoteJob,
    #[serde(default)]
    pub outputs: Vec<GenerationResponse>,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunEvent {
    Started {
        request_id: String,
    },
    PartialImage {
        index: u32,
        image: AssetSource,
        raw: Value,
    },
    TextDelta {
        text: String,
        raw: Value,
    },
    Checkpoint {
        event_id: String,
        status: Option<String>,
        raw: Value,
    },
    Progress {
        status: String,
        raw: Value,
    },
    Completed {
        response: GenerationResponse,
    },
}

pub type EventHandler = Arc<dyn Fn(RunEvent) + Send + Sync>;

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct DownloadedAsset {
    pub bytes: Vec<u8>,
    pub mime_type: Option<String>,
    pub filename: Option<String>,
}
