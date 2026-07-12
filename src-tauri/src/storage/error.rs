use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
pub enum StorageError {
    Io(std::io::Error),
    Database(sqlx::Error),
    Serialization(serde_json::Error),
    InvalidProject(String),
    InvalidPath(PathBuf),
    NotFound(String),
    Conflict(String),
}

pub type StorageResult<T> = Result<T, StorageError>;

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "file operation failed: {error}"),
            Self::Database(error) => write!(f, "database operation failed: {error}"),
            Self::Serialization(error) => write!(f, "stored data is invalid: {error}"),
            Self::InvalidProject(message) => write!(f, "invalid project: {message}"),
            Self::InvalidPath(path) => write!(f, "path is outside the project: {}", path.display()),
            Self::NotFound(entity) => write!(f, "not found: {entity}"),
            Self::Conflict(message) => write!(f, "conflict: {message}"),
        }
    }
}

impl std::error::Error for StorageError {}

impl From<std::io::Error> for StorageError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<sqlx::Error> for StorageError {
    fn from(value: sqlx::Error) -> Self {
        Self::Database(value)
    }
}

impl From<serde_json::Error> for StorageError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serialization(value)
    }
}
