use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ProviderErrorKind {
    Configuration,
    Validation,
    Authentication,
    Permission,
    RateLimit,
    Http,
    Api,
    Parse,
    Unsupported,
    Io,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ProviderError {
    pub kind: ProviderErrorKind,
    pub message: String,
    pub status: Option<u16>,
    pub code: Option<String>,
    pub request_id: Option<String>,
    pub retry_after_seconds: Option<u64>,
    pub details: Option<Value>,
}

impl ProviderError {
    pub fn new(kind: ProviderErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            status: None,
            code: None,
            request_id: None,
            retry_after_seconds: None,
            details: None,
        }
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::Validation, message)
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::Unsupported, message)
    }

    pub fn io(error: impl Display) -> Self {
        Self::new(ProviderErrorKind::Io, error.to_string())
    }

    pub fn parse(message: impl Into<String>, details: Option<Value>) -> Self {
        let mut error = Self::new(ProviderErrorKind::Parse, message);
        error.details = details;
        error
    }
}

impl Display for ProviderError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        if let Some(status) = self.status {
            write!(formatter, "{} (HTTP {status})", self.message)
        } else {
            formatter.write_str(&self.message)
        }
    }
}

impl std::error::Error for ProviderError {}

impl From<reqwest::Error> for ProviderError {
    fn from(error: reqwest::Error) -> Self {
        let mut result = ProviderError::new(ProviderErrorKind::Http, error.to_string());
        result.status = error.status().map(|status| status.as_u16());
        result
    }
}

impl From<serde_json::Error> for ProviderError {
    fn from(error: serde_json::Error) -> Self {
        ProviderError::new(ProviderErrorKind::Parse, error.to_string())
    }
}
