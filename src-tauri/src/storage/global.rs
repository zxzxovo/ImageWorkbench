use std::path::Path;

use chrono::Utc;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row, SqlitePool};

use crate::domain::{ProjectSummary, ProviderKind, ProviderProfile};

use super::migrations::{GLOBAL_MIGRATIONS, migrate};
use super::{StorageError, StorageResult};

#[derive(Debug, Clone)]
pub struct GlobalStore {
    pool: SqlitePool,
}

impl GlobalStore {
    pub async fn open(path: impl AsRef<Path>) -> StorageResult<Self> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal);
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;
        migrate(&pool, GLOBAL_MIGRATIONS).await?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn upsert_provider(&self, profile: &ProviderProfile) -> StorageResult<()> {
        let json = serde_json::to_string(profile)?;
        sqlx::query(
            r#"INSERT INTO provider_profiles
               (id, name, kind, credential_ref, profile_json, created_at, updated_at)
               VALUES(?, ?, ?, ?, ?, ?, ?)
               ON CONFLICT(id) DO UPDATE SET
                 name = excluded.name,
                 kind = excluded.kind,
                 credential_ref = excluded.credential_ref,
                 profile_json = excluded.profile_json,
                 updated_at = excluded.updated_at"#,
        )
        .bind(&profile.id)
        .bind(&profile.name)
        .bind(provider_kind_key(&profile.kind))
        .bind(&profile.credential_ref)
        .bind(json)
        .bind(profile.created_at.to_rfc3339())
        .bind(profile.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn provider(&self, id: &str) -> StorageResult<Option<ProviderProfile>> {
        let json: Option<String> =
            sqlx::query_scalar("SELECT profile_json FROM provider_profiles WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        deserialize_optional(json)
    }

    pub async fn list_providers(&self) -> StorageResult<Vec<ProviderProfile>> {
        let rows = sqlx::query("SELECT profile_json FROM provider_profiles ORDER BY name, id")
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|row| {
                serde_json::from_str(row.get::<String, _>("profile_json").as_str())
                    .map_err(Into::into)
            })
            .collect()
    }

    pub async fn delete_provider(&self, id: &str) -> StorageResult<bool> {
        let result = sqlx::query("DELETE FROM provider_profiles WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn set_setting<T: Serialize + ?Sized>(
        &self,
        key: &str,
        value: &T,
    ) -> StorageResult<()> {
        let value = serde_json::to_string(value)?;
        sqlx::query(
            r#"INSERT INTO app_settings(key, value_json, updated_at) VALUES(?, ?, ?)
               ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json, updated_at = excluded.updated_at"#,
        )
        .bind(key)
        .bind(value)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn setting<T: DeserializeOwned>(&self, key: &str) -> StorageResult<Option<T>> {
        let value: Option<String> =
            sqlx::query_scalar("SELECT value_json FROM app_settings WHERE key = ?")
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;
        deserialize_optional(value)
    }

    pub async fn add_recent_project(&self, summary: &ProjectSummary) -> StorageResult<()> {
        let root = summary.root_path.to_string_lossy().into_owned();
        let json = serde_json::to_string(summary)?;
        sqlx::query(
            r#"INSERT INTO recent_projects(project_id, root_path, name, summary_json, last_opened_at)
               VALUES(?, ?, ?, ?, ?)
               ON CONFLICT(project_id) DO UPDATE SET
                 root_path = excluded.root_path,
                 name = excluded.name,
                 summary_json = excluded.summary_json,
                 last_opened_at = excluded.last_opened_at"#,
        )
        .bind(&summary.id)
        .bind(root)
        .bind(&summary.name)
        .bind(json)
        .bind(summary.last_opened_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_recent_projects(&self, limit: u32) -> StorageResult<Vec<ProjectSummary>> {
        let rows = sqlx::query(
            "SELECT summary_json FROM recent_projects ORDER BY last_opened_at DESC LIMIT ?",
        )
        .bind(i64::from(limit.min(100)))
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                serde_json::from_str(row.get::<String, _>("summary_json").as_str())
                    .map_err(Into::into)
            })
            .collect()
    }

    pub async fn remove_recent_project(&self, project_id: &str) -> StorageResult<bool> {
        let result = sqlx::query("DELETE FROM recent_projects WHERE project_id = ?")
            .bind(project_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn set_capability_override(
        &self,
        provider_profile_id: &str,
        model_id: &str,
        patch: &Value,
    ) -> StorageResult<()> {
        if !patch.is_object() {
            return Err(StorageError::Conflict(
                "a capability override must be a JSON object".to_owned(),
            ));
        }
        sqlx::query(
            r#"INSERT INTO model_capability_overrides
               (provider_profile_id, model_id, patch_json, updated_at) VALUES(?, ?, ?, ?)
               ON CONFLICT(provider_profile_id, model_id) DO UPDATE SET
                 patch_json = excluded.patch_json, updated_at = excluded.updated_at"#,
        )
        .bind(provider_profile_id)
        .bind(model_id)
        .bind(serde_json::to_string(patch)?)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn capability_override(
        &self,
        provider_profile_id: &str,
        model_id: &str,
    ) -> StorageResult<Option<Value>> {
        let patch: Option<String> = sqlx::query_scalar(
            "SELECT patch_json FROM model_capability_overrides WHERE provider_profile_id = ? AND model_id = ?",
        )
        .bind(provider_profile_id)
        .bind(model_id)
        .fetch_optional(&self.pool)
        .await?;
        deserialize_optional(patch)
    }
}

fn deserialize_optional<T: DeserializeOwned>(value: Option<String>) -> StorageResult<Option<T>> {
    value
        .map(|value| serde_json::from_str(&value).map_err(Into::into))
        .transpose()
}

fn provider_kind_key(kind: &ProviderKind) -> &'static str {
    match kind {
        ProviderKind::OpenAi => "open_ai",
        ProviderKind::XAi => "xai",
        ProviderKind::Gemini => "gemini",
        ProviderKind::OpenAiCompatible => "open_ai_compatible",
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use chrono::Utc;
    use serde_json::json;
    use uuid::Uuid;

    use super::*;

    fn database_path(name: &str) -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(format!(
            "imageworkbench-global-{name}-{}.sqlite3",
            Uuid::new_v4()
        ));
        (directory, path)
    }

    #[tokio::test]
    async fn persists_profiles_without_secret_values_and_settings() {
        let (_directory, path) = database_path("profiles");
        let store = GlobalStore::open(&path).await.unwrap();
        let mut profile =
            ProviderProfile::new("OpenAI", ProviderKind::OpenAi, "https://api.openai.com/v1");
        profile.credential_ref = Some("keyring:provider/openai".to_owned());
        store.upsert_provider(&profile).await.unwrap();
        store.set_setting("locale", &json!("zh-CN")).await.unwrap();

        assert_eq!(store.provider(&profile.id).await.unwrap(), Some(profile));
        assert_eq!(
            store.setting::<Value>("locale").await.unwrap(),
            Some(json!("zh-CN"))
        );

        store.pool.close().await;
        drop(store);
    }

    #[tokio::test]
    async fn stores_recent_projects_in_opened_order() {
        let (_directory, path) = database_path("recent");
        let store = GlobalStore::open(&path).await.unwrap();
        let summary = ProjectSummary {
            id: Uuid::new_v4().to_string(),
            name: "Example".to_owned(),
            root_path: PathBuf::from("C:/projects/example"),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_opened_at: Utc::now(),
            default_provider_profile_id: None,
            default_model_id: None,
            default_parameters: Default::default(),
        };
        store.add_recent_project(&summary).await.unwrap();

        assert_eq!(store.list_recent_projects(10).await.unwrap(), vec![summary]);

        store.pool.close().await;
        drop(store);
    }
}
