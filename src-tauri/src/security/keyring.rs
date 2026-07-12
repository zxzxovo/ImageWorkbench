use std::collections::HashMap;
use std::fmt;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CredentialKey {
    pub service: String,
    pub account: String,
}

impl CredentialKey {
    pub fn provider(provider_profile_id: &str) -> Self {
        Self {
            service: "dev.imageworkbench.desktop".to_owned(),
            account: format!("provider:{provider_profile_id}"),
        }
    }

    /// This opaque reference is safe to store in SQLite and project exports.
    pub fn reference(&self) -> String {
        format!("keyring://{}/{}", self.service, self.account)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyringError {
    NotFound,
    Unavailable(String),
    InvalidSecret,
    Poisoned,
}

impl fmt::Display for KeyringError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(f, "credential not found"),
            Self::Unavailable(message) => {
                write!(f, "system credential store unavailable: {message}")
            }
            Self::InvalidSecret => write!(f, "credential cannot be empty"),
            Self::Poisoned => write!(f, "credential store lock is poisoned"),
        }
    }
}

impl std::error::Error for KeyringError {}

pub trait Keyring: Send + Sync {
    fn set(&self, key: &CredentialKey, secret: &str) -> Result<(), KeyringError>;
    fn get(&self, key: &CredentialKey) -> Result<String, KeyringError>;
    fn delete(&self, key: &CredentialKey) -> Result<(), KeyringError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemKeyring;

impl Keyring for SystemKeyring {
    fn set(&self, key: &CredentialKey, secret: &str) -> Result<(), KeyringError> {
        if secret.is_empty() {
            return Err(KeyringError::InvalidSecret);
        }
        entry(key)?.set_password(secret).map_err(map_system_error)
    }

    fn get(&self, key: &CredentialKey) -> Result<String, KeyringError> {
        entry(key)?.get_password().map_err(map_system_error)
    }

    fn delete(&self, key: &CredentialKey) -> Result<(), KeyringError> {
        entry(key)?.delete_credential().map_err(map_system_error)
    }
}

fn entry(key: &CredentialKey) -> Result<keyring::Entry, KeyringError> {
    keyring::Entry::new(&key.service, &key.account).map_err(map_system_error)
}

fn map_system_error(error: keyring::Error) -> KeyringError {
    match error {
        keyring::Error::NoEntry => KeyringError::NotFound,
        other => KeyringError::Unavailable(other.to_string()),
    }
}

#[derive(Debug, Default)]
pub struct MemoryKeyring {
    values: RwLock<HashMap<CredentialKey, String>>,
}

impl Keyring for MemoryKeyring {
    fn set(&self, key: &CredentialKey, secret: &str) -> Result<(), KeyringError> {
        if secret.is_empty() {
            return Err(KeyringError::InvalidSecret);
        }
        self.values
            .write()
            .map_err(|_| KeyringError::Poisoned)?
            .insert(key.clone(), secret.to_owned());
        Ok(())
    }

    fn get(&self, key: &CredentialKey) -> Result<String, KeyringError> {
        self.values
            .read()
            .map_err(|_| KeyringError::Poisoned)?
            .get(key)
            .cloned()
            .ok_or(KeyringError::NotFound)
    }

    fn delete(&self, key: &CredentialKey) -> Result<(), KeyringError> {
        let removed = self
            .values
            .write()
            .map_err(|_| KeyringError::Poisoned)?
            .remove(key);
        if removed.is_some() {
            Ok(())
        } else {
            Err(KeyringError::NotFound)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_keyring_matches_system_contract() {
        let store = MemoryKeyring::default();
        let key = CredentialKey::provider("provider-id");

        assert_eq!(store.get(&key), Err(KeyringError::NotFound));
        store.set(&key, "secret").unwrap();
        assert_eq!(store.get(&key).unwrap(), "secret");
        store.delete(&key).unwrap();
        assert_eq!(store.get(&key), Err(KeyringError::NotFound));
        assert!(!key.reference().contains("secret"));
    }

    #[test]
    fn rejects_empty_secrets() {
        let store = MemoryKeyring::default();
        let key = CredentialKey::provider("provider-id");
        assert_eq!(store.set(&key, ""), Err(KeyringError::InvalidSecret));
    }
}
