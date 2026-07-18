use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::{Notify, RwLock, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::bridge::{CommandError, CommandResult, ProviderProfileDto};
use crate::domain::ProjectSummary;
use crate::security::{CredentialKey, Keyring, KeyringError, SystemKeyring};
use crate::storage::{GlobalStore, ProjectStore, stage_project_move, strip_extended_length_prefix};

const DEFAULT_PROVIDER_CONCURRENCY: usize = 2;

#[derive(Default)]
pub struct QueueGate {
    paused: AtomicBool,
    notify: Notify,
}

impl QueueGate {
    pub fn pause(&self) {
        self.paused.store(true, Ordering::Release);
    }

    pub fn resume(&self) {
        if self.paused.swap(false, Ordering::AcqRel) {
            self.notify.notify_waiters();
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Acquire)
    }

    pub async fn wait(&self, token: &CancellationToken) -> bool {
        loop {
            let notified = self.notify.notified();
            if !self.is_paused() {
                return true;
            }
            tokio::select! {
                _ = token.cancelled() => return false,
                _ = notified => {}
            }
        }
    }
}

pub struct AppState {
    global_store: GlobalStore,
    keyring: Arc<dyn Keyring>,
    projects: RwLock<HashMap<String, Arc<ProjectStore>>>,
    provider_limits: RwLock<HashMap<String, Arc<Semaphore>>>,
    cancellations: RwLock<HashMap<String, CancellationToken>>,
    queue_gate: Arc<QueueGate>,
}

impl AppState {
    pub async fn open(app_data_directory: impl AsRef<Path>) -> CommandResult<Self> {
        std::fs::create_dir_all(app_data_directory.as_ref())?;
        let global_store =
            GlobalStore::open(app_data_directory.as_ref().join("imageworkbench.sqlite3")).await?;
        Ok(Self::new(global_store, Arc::new(SystemKeyring)))
    }

    pub fn new(global_store: GlobalStore, keyring: Arc<dyn Keyring>) -> Self {
        Self {
            global_store,
            keyring,
            projects: RwLock::new(HashMap::new()),
            provider_limits: RwLock::new(HashMap::new()),
            cancellations: RwLock::new(HashMap::new()),
            queue_gate: Arc::new(QueueGate::default()),
        }
    }

    pub fn global_store(&self) -> &GlobalStore {
        &self.global_store
    }

    pub async fn resolved_provider_config(
        &self,
        profile: &ProviderProfileDto,
    ) -> CommandResult<crate::providers::types::ProviderConfig> {
        let mut resolved = profile.clone();
        for header in &mut resolved.custom_headers {
            if header.secret {
                header.value = self.secret(header.credential_key(&profile.id)).await?;
            }
        }
        resolved.to_provider_config()
    }

    pub async fn set_secret(&self, key: CredentialKey, secret: String) -> CommandResult<()> {
        if secret.is_empty() {
            return Err(CommandError::validation("API key cannot be empty"));
        }
        let keyring = Arc::clone(&self.keyring);
        let account = key.account.clone();
        let result = tokio::task::spawn_blocking(move || keyring.set(&key, &secret))
            .await
            .map_err(|error| CommandError::new("keyring_task", error.to_string()))?
            .map_err(map_keyring_error);
        if let Err(error) = &result {
            tracing::error!(code = %error.code, %account, "failed to store credential");
        }
        result
    }

    pub async fn secret(&self, key: CredentialKey) -> CommandResult<String> {
        let keyring = Arc::clone(&self.keyring);
        tokio::task::spawn_blocking(move || keyring.get(&key))
            .await
            .map_err(|error| CommandError::new("keyring_task", error.to_string()))?
            .map_err(map_keyring_error)
    }

    pub async fn delete_secret(&self, key: CredentialKey) -> CommandResult<()> {
        let keyring = Arc::clone(&self.keyring);
        tokio::task::spawn_blocking(move || keyring.delete(&key))
            .await
            .map_err(|error| CommandError::new("keyring_task", error.to_string()))?
            .map_err(map_keyring_error)
    }

    pub async fn credential_store_health(&self) -> CommandResult<()> {
        let keyring = Arc::clone(&self.keyring);
        tokio::task::spawn_blocking(move || {
            let token = uuid::Uuid::new_v4().to_string();
            let key = CredentialKey {
                service: "dev.imageworkbench.desktop".to_owned(),
                account: format!("diagnostic:{token}"),
            };
            let operation = (|| {
                keyring.set(&key, &token)?;
                let stored = keyring.get(&key)?;
                if stored != token {
                    return Err(KeyringError::Unavailable(
                        "credential round-trip returned a different value".to_owned(),
                    ));
                }
                Ok(())
            })();
            let cleanup = keyring.delete(&key);
            match operation {
                Err(error) => {
                    let _ = cleanup;
                    Err(error)
                }
                Ok(()) => cleanup,
            }
        })
        .await
        .map_err(|error| CommandError::new("keyring_task", error.to_string()))?
        .map_err(map_keyring_error)
    }

    pub async fn open_project(&self, root: impl AsRef<Path>) -> CommandResult<Arc<ProjectStore>> {
        let store = Arc::new(ProjectStore::open(root).await?);
        store.mark_local_work_interrupted().await?;
        let summary = store.summary().await?;
        self.global_store.add_recent_project(&summary).await?;
        self.projects
            .write()
            .await
            .insert(summary.id.clone(), Arc::clone(&store));
        Ok(store)
    }

    pub async fn create_project(
        &self,
        project_id: &str,
        name: &str,
        root: impl AsRef<Path>,
    ) -> CommandResult<Arc<ProjectStore>> {
        validate_identifier(project_id, "project ID")?;
        if name.trim().is_empty() {
            return Err(CommandError::validation("project name is required"));
        }
        let store = Arc::new(ProjectStore::create(root, name).await?);
        let mut summary = store.summary().await?;
        summary.id = project_id.to_owned();
        summary.name = name.trim().to_owned();
        summary.updated_at = chrono::Utc::now();
        store.save_summary(&summary).await?;
        self.global_store.add_recent_project(&summary).await?;
        self.projects
            .write()
            .await
            .insert(project_id.to_owned(), Arc::clone(&store));
        Ok(store)
    }

    pub async fn open_or_create_project(
        &self,
        project_id: &str,
        name: &str,
        root: impl AsRef<Path>,
    ) -> CommandResult<Arc<ProjectStore>> {
        validate_identifier(project_id, "project ID")?;
        let requested_root = root.as_ref();
        if let Some(store) = self.projects.read().await.get(project_id).cloned() {
            let expected =
                strip_extended_length_prefix(requested_root.canonicalize().map_err(|error| {
                    CommandError::new(
                        "project_path",
                        format!("cannot resolve {}: {error}", requested_root.display()),
                    )
                })?);
            if store.layout().root() != expected {
                return Err(CommandError::validation(
                    "the requested storage path does not match the open project",
                ));
            }
            return Ok(store);
        }

        let database = requested_root
            .join(".imageworkbench")
            .join("project.sqlite3");
        if database.is_file() {
            let store = self.open_project(requested_root).await?;
            let summary = store.summary().await?;
            if summary.id != project_id {
                self.projects.write().await.remove(&summary.id);
                return Err(CommandError::validation(format!(
                    "storage path belongs to project {}, not {project_id}",
                    summary.id
                )));
            }
            Ok(store)
        } else {
            self.create_project(project_id, name, requested_root).await
        }
    }

    pub async fn project(&self, project_id: &str) -> CommandResult<Arc<ProjectStore>> {
        self.projects
            .read()
            .await
            .get(project_id)
            .cloned()
            .ok_or_else(|| CommandError::not_found(format!("project {project_id} is not open")))
    }

    pub async fn close_project(&self, project_id: &str) -> bool {
        self.projects.write().await.remove(project_id).is_some()
    }

    pub async fn move_project(
        &self,
        project_id: &str,
        destination: impl AsRef<Path>,
    ) -> CommandResult<(Arc<ProjectStore>, PathBuf)> {
        let source = self.project(project_id).await?;
        if source.has_active_work().await? {
            return Err(CommandError::new(
                "project_busy",
                "project cannot be moved while local or remote generation work is active",
            ));
        }
        let source_root = source.layout().root().to_owned();
        self.projects.write().await.remove(project_id);
        let moved = match stage_project_move(&source, destination).await {
            Ok(store) => Arc::new(store),
            Err(error) => {
                self.projects
                    .write()
                    .await
                    .insert(project_id.to_owned(), Arc::clone(&source));
                return Err(error.into());
            }
        };
        let summary = moved.summary().await?;
        if summary.id != project_id {
            self.projects
                .write()
                .await
                .insert(project_id.to_owned(), Arc::clone(&source));
            return Err(CommandError::new(
                "project_id_mismatch",
                "moved project identity does not match the requested project",
            ));
        }
        source.pool().close().await;
        self.projects
            .write()
            .await
            .insert(project_id.to_owned(), Arc::clone(&moved));
        self.global_store.add_recent_project(&summary).await?;
        Ok((moved, source_root))
    }

    pub async fn open_project_summaries(&self) -> CommandResult<Vec<ProjectSummary>> {
        let stores = self
            .projects
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut summaries = Vec::with_capacity(stores.len());
        for store in stores {
            summaries.push(store.summary().await?);
        }
        summaries.sort_by(|left, right| right.last_opened_at.cmp(&left.last_opened_at));
        Ok(summaries)
    }

    pub async fn open_project_stores(&self) -> Vec<Arc<ProjectStore>> {
        self.projects.read().await.values().cloned().collect()
    }

    pub async fn provider_limit(&self, provider_profile_id: &str) -> Arc<Semaphore> {
        if let Some(limit) = self
            .provider_limits
            .read()
            .await
            .get(provider_profile_id)
            .cloned()
        {
            return limit;
        }
        let mut limits = self.provider_limits.write().await;
        Arc::clone(
            limits
                .entry(provider_profile_id.to_owned())
                .or_insert_with(|| Arc::new(Semaphore::new(DEFAULT_PROVIDER_CONCURRENCY))),
        )
    }

    pub fn queue_gate(&self) -> Arc<QueueGate> {
        Arc::clone(&self.queue_gate)
    }

    pub fn pause_queue(&self) {
        self.queue_gate.pause();
    }

    pub fn resume_queue(&self) {
        self.queue_gate.resume();
    }

    pub fn queue_is_paused(&self) -> bool {
        self.queue_gate.is_paused()
    }

    pub async fn register_cancellation(&self, run_id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        self.cancellations
            .write()
            .await
            .insert(run_id.to_owned(), token.clone());
        token
    }

    pub async fn cancel(&self, run_id: &str) -> bool {
        if let Some(token) = self.cancellations.read().await.get(run_id) {
            token.cancel();
            true
        } else {
            false
        }
    }

    pub async fn finish_cancellation(&self, run_id: &str) {
        self.cancellations.write().await.remove(run_id);
    }

    pub async fn validate_open_project_path(
        &self,
        path: impl AsRef<Path>,
    ) -> CommandResult<PathBuf> {
        let canonical =
            strip_extended_length_prefix(path.as_ref().canonicalize().map_err(|error| {
                CommandError::new(
                    "invalid_path",
                    format!("cannot resolve {}: {error}", path.as_ref().display()),
                )
            })?);
        let roots = self
            .projects
            .read()
            .await
            .values()
            .map(|store| store.layout().root().to_owned())
            .collect::<Vec<_>>();
        if roots.iter().any(|root| canonical.starts_with(root)) {
            Ok(canonical)
        } else {
            Err(CommandError::validation(
                "path is not inside an open project",
            ))
        }
    }

    pub async fn shutdown(&self) -> CommandResult<()> {
        let stores = self
            .projects
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for store in stores {
            store.mark_local_work_interrupted().await?;
        }
        Ok(())
    }
}

fn validate_identifier(value: &str, label: &str) -> CommandResult<()> {
    if value.trim().is_empty() || value.len() > 128 {
        return Err(CommandError::validation(format!("{label} is invalid")));
    }
    Ok(())
}

fn map_keyring_error(error: KeyringError) -> CommandError {
    match error {
        KeyringError::NotFound => CommandError::not_found("API key is not configured"),
        KeyringError::InvalidSecret => CommandError::validation("API key cannot be empty"),
        KeyringError::Unavailable(message) => CommandError::new("keyring_unavailable", message),
        KeyringError::Poisoned => {
            CommandError::new("keyring_unavailable", "keyring lock is poisoned")
        }
    }
}

impl From<std::io::Error> for CommandError {
    fn from(error: std::io::Error) -> Self {
        Self::new("io", error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use crate::security::MemoryKeyring;

    use super::*;

    #[tokio::test]
    async fn stores_secrets_outside_sqlite_and_limits_providers() {
        let directory = tempfile::tempdir().unwrap();
        let global = GlobalStore::open(directory.path().join("global.sqlite3"))
            .await
            .unwrap();
        let state = AppState::new(global, Arc::new(MemoryKeyring::default()));
        let key = CredentialKey::provider("provider");

        state
            .set_secret(key.clone(), "secret".to_owned())
            .await
            .unwrap();

        assert_eq!(state.secret(key).await.unwrap(), "secret");
        state.credential_store_health().await.unwrap();
        assert_eq!(
            state.provider_limit("provider").await.available_permits(),
            2
        );
        state.global_store.pool().close().await;
    }

    #[tokio::test]
    async fn creates_and_tracks_portable_projects() {
        let directory = tempfile::tempdir().unwrap();
        let global = GlobalStore::open(directory.path().join("global.sqlite3"))
            .await
            .unwrap();
        let state = AppState::new(global, Arc::new(MemoryKeyring::default()));
        let root = directory.path().join("project");

        let store = state
            .open_or_create_project("project-id", "Example", &root)
            .await
            .unwrap();

        assert_eq!(store.summary().await.unwrap().id, "project-id");
        assert_eq!(state.open_project_summaries().await.unwrap().len(), 1);
        assert!(
            state
                .validate_open_project_path(store.layout().root())
                .await
                .is_ok()
        );
        state.global_store.pool().close().await;
        store.pool().close().await;
    }

    #[tokio::test]
    async fn refuses_to_create_a_project_in_an_occupied_directory() {
        let directory = tempfile::tempdir().unwrap();
        let global = GlobalStore::open(directory.path().join("global.sqlite3"))
            .await
            .unwrap();
        let state = AppState::new(global, Arc::new(MemoryKeyring::default()));
        let root = directory.path().join("occupied");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("user-file.txt"), b"keep").unwrap();

        let error = state
            .create_project("project-id", "Example", &root)
            .await
            .unwrap_err();

        assert!(error.message.contains("not empty"));
        assert!(root.join("user-file.txt").is_file());
        state.global_store.pool().close().await;
    }

    #[tokio::test]
    async fn queue_gate_waits_until_resumed() {
        let gate = Arc::new(QueueGate::default());
        let token = CancellationToken::new();
        gate.pause();

        let waiter = tokio::spawn({
            let gate = Arc::clone(&gate);
            let token = token.clone();
            async move { gate.wait(&token).await }
        });
        tokio::task::yield_now().await;
        assert!(!waiter.is_finished());

        gate.resume();
        assert!(waiter.await.unwrap());
    }

    #[tokio::test]
    async fn queue_gate_returns_immediately_when_running() {
        let gate = QueueGate::default();
        let token = CancellationToken::new();

        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), gate.wait(&token))
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn queue_gate_wait_is_cancelled_while_paused() {
        let gate = Arc::new(QueueGate::default());
        let token = CancellationToken::new();
        gate.pause();

        let waiter = tokio::spawn({
            let gate = Arc::clone(&gate);
            let token = token.clone();
            async move { gate.wait(&token).await }
        });
        tokio::task::yield_now().await;
        token.cancel();

        assert!(!waiter.await.unwrap());
    }
}
