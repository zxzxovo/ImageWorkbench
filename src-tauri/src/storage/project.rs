use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::domain::{
    ErrorRecord, GenerationPreset, InputAsset, JobRecord, JobStatus, OutputPart, ProjectSummary,
    PromptContext, RunRecord, RunStatus, UsageRecord,
};

use super::migrations::{PROJECT_MIGRATIONS, migrate};
use super::{ProjectLayout, StorageError, StorageResult, strip_extended_length_prefix};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTaskRecord {
    pub id: String,
    pub run_id: String,
    pub job_id: Option<String>,
    pub provider_profile_id: String,
    pub remote_id: String,
    pub task_type: String,
    pub status: String,
    pub next_poll_at: Option<DateTime<Utc>>,
    #[serde(default)]
    #[specta(type = std::collections::BTreeMap<String, specta_typescript::Unknown>)]
    pub metadata: BTreeMap<String, Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFileRecord {
    pub id: String,
    pub provider_profile_id: String,
    pub provider_file_id: String,
    pub purpose: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    #[specta(type = std::collections::BTreeMap<String, specta_typescript::Unknown>)]
    pub metadata: BTreeMap<String, Value>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RunDeleteResult {
    pub deleted: bool,
    pub local_assets_deleted: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRemapResult {
    pub summaries: u32,
    pub presets: u32,
    pub conversations: u32,
    pub runs: u32,
    pub jobs: u32,
    pub remote_tasks: u32,
    pub remote_files: u32,
}

#[derive(Debug, Clone)]
pub struct ProjectStore {
    layout: ProjectLayout,
    pool: SqlitePool,
}

impl ProjectStore {
    pub async fn create(root: impl AsRef<Path>, name: impl Into<String>) -> StorageResult<Self> {
        let layout = ProjectLayout::create(root)?;
        let pool = connect(&layout).await?;
        migrate(&pool, PROJECT_MIGRATIONS).await?;
        let now = Utc::now();
        let summary = ProjectSummary {
            id: Uuid::new_v4().to_string(),
            name: name.into(),
            root_path: layout.root().to_owned(),
            created_at: now,
            updated_at: now,
            last_opened_at: now,
            default_provider_profile_id: None,
            default_model_id: None,
            default_parameters: BTreeMap::new(),
        };
        let store = Self { layout, pool };
        store.save_summary(&summary).await?;
        Ok(store)
    }

    pub async fn open(root: impl AsRef<Path>) -> StorageResult<Self> {
        let layout = ProjectLayout::open(root)?;
        let pool = connect(&layout).await?;
        migrate(&pool, PROJECT_MIGRATIONS).await?;
        let store = Self { layout, pool };
        let mut summary = store.summary().await?;
        summary.root_path = store.layout.root().to_owned();
        summary.last_opened_at = Utc::now();
        summary.updated_at = Utc::now();
        store.save_summary(&summary).await?;
        Ok(store)
    }

    pub fn layout(&self) -> &ProjectLayout {
        &self.layout
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn summary(&self) -> StorageResult<ProjectSummary> {
        let json: Option<String> =
            sqlx::query_scalar("SELECT summary_json FROM project_metadata WHERE singleton = 1")
                .fetch_optional(&self.pool)
                .await?;
        deserialize_required(json, "project metadata")
    }

    pub async fn save_summary(&self, summary: &ProjectSummary) -> StorageResult<()> {
        let mut portable = summary.clone();
        portable.root_path = self.layout.root().to_owned();
        sqlx::query(
            r#"INSERT INTO project_metadata(singleton, summary_json, updated_at) VALUES(1, ?, ?)
               ON CONFLICT(singleton) DO UPDATE SET
                 summary_json = excluded.summary_json, updated_at = excluded.updated_at"#,
        )
        .bind(serde_json::to_string(&portable)?)
        .bind(portable.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn upsert_prompt_context(&self, context: &PromptContext) -> StorageResult<()> {
        sqlx::query(
            r#"INSERT INTO prompt_contexts
               (id, name, enabled, sort_order, context_json, created_at, updated_at)
               VALUES(?, ?, ?, ?, ?, ?, ?)
               ON CONFLICT(id) DO UPDATE SET
                 name = excluded.name,
                 enabled = excluded.enabled,
                 sort_order = excluded.sort_order,
                 context_json = excluded.context_json,
                 updated_at = excluded.updated_at"#,
        )
        .bind(&context.id)
        .bind(&context.name)
        .bind(context.enabled)
        .bind(context.sort_order)
        .bind(serde_json::to_string(context)?)
        .bind(context.created_at.to_rfc3339())
        .bind(context.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_prompt_contexts(&self) -> StorageResult<Vec<PromptContext>> {
        fetch_json_list(
            &self.pool,
            "SELECT context_json FROM prompt_contexts ORDER BY sort_order, id",
            "context_json",
        )
        .await
    }

    pub async fn delete_prompt_context(&self, id: &str) -> StorageResult<bool> {
        delete_by_id(&self.pool, "prompt_contexts", id).await
    }

    pub async fn upsert_preset(&self, preset: &GenerationPreset) -> StorageResult<()> {
        sqlx::query(
            r#"INSERT INTO generation_presets(id, name, preset_json, created_at, updated_at)
               VALUES(?, ?, ?, ?, ?)
               ON CONFLICT(id) DO UPDATE SET
                 name = excluded.name, preset_json = excluded.preset_json, updated_at = excluded.updated_at"#,
        )
        .bind(&preset.id)
        .bind(&preset.name)
        .bind(serde_json::to_string(preset)?)
        .bind(preset.created_at.to_rfc3339())
        .bind(preset.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_presets(&self) -> StorageResult<Vec<GenerationPreset>> {
        fetch_json_list(
            &self.pool,
            "SELECT preset_json FROM generation_presets ORDER BY name, id",
            "preset_json",
        )
        .await
    }

    pub async fn delete_preset(&self, id: &str) -> StorageResult<bool> {
        delete_by_id(&self.pool, "generation_presets", id).await
    }

    pub async fn save_conversation(
        &self,
        id: &str,
        title: &str,
        provider_profile_id: Option<&str>,
        model_id: Option<&str>,
        state: &Value,
    ) -> StorageResult<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"INSERT INTO conversations
               (id, title, provider_profile_id, model_id, state_json, created_at, updated_at)
               VALUES(?, ?, ?, ?, ?, ?, ?)
               ON CONFLICT(id) DO UPDATE SET
                 title = excluded.title,
                 provider_profile_id = excluded.provider_profile_id,
                 model_id = excluded.model_id,
                 state_json = excluded.state_json,
                 updated_at = excluded.updated_at"#,
        )
        .bind(id)
        .bind(title)
        .bind(provider_profile_id)
        .bind(model_id)
        .bind(serde_json::to_string(state)?)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn conversation(&self, id: &str) -> StorageResult<Option<Value>> {
        let json: Option<String> =
            sqlx::query_scalar("SELECT state_json FROM conversations WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        deserialize_optional(json)
    }

    pub async fn create_run_group(
        &self,
        id: &str,
        name: Option<&str>,
        metadata: &Value,
    ) -> StorageResult<()> {
        sqlx::query(
            "INSERT INTO run_groups(id, name, metadata_json, created_at) VALUES(?, ?, ?, ?)",
        )
        .bind(id)
        .bind(name)
        .bind(serde_json::to_string(metadata)?)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn upsert_run(&self, run: &RunRecord) -> StorageResult<()> {
        sqlx::query(
            r#"INSERT INTO runs
               (id, group_id, provider_profile_id, model_id, operation, execution_mode, status, run_json, created_at, updated_at)
               VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
               ON CONFLICT(id) DO UPDATE SET
                 group_id = excluded.group_id,
                 provider_profile_id = excluded.provider_profile_id,
                 model_id = excluded.model_id,
                 operation = excluded.operation,
                 execution_mode = excluded.execution_mode,
                 status = excluded.status,
                 run_json = excluded.run_json,
                 updated_at = excluded.updated_at"#,
        )
        .bind(&run.id)
        .bind(&run.group_id)
        .bind(&run.request.provider_profile_id)
        .bind(&run.request.model_id)
        .bind(enum_key(&run.request.operation)?)
        .bind(enum_key(&run.request.execution_mode)?)
        .bind(enum_key(&run.status)?)
        .bind(serde_json::to_string(run)?)
        .bind(run.created_at.to_rfc3339())
        .bind(run.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn run(&self, id: &str) -> StorageResult<Option<RunRecord>> {
        fetch_json_by_id(&self.pool, "runs", "run_json", id).await
    }

    pub async fn list_runs(&self, limit: u32, offset: u32) -> StorageResult<Vec<RunRecord>> {
        let rows =
            sqlx::query("SELECT run_json FROM runs ORDER BY created_at DESC LIMIT ? OFFSET ?")
                .bind(i64::from(limit.clamp(1, 500)))
                .bind(i64::from(offset))
                .fetch_all(&self.pool)
                .await?;
        deserialize_rows(rows, "run_json")
    }

    pub async fn run_ids(&self) -> StorageResult<Vec<String>> {
        sqlx::query_scalar("SELECT id FROM runs ORDER BY created_at DESC, id")
            .fetch_all(&self.pool)
            .await
            .map_err(Into::into)
    }

    pub async fn validate_run_local_assets(&self, run_id: &str) -> StorageResult<()> {
        if self.run(run_id).await?.is_none() {
            return Err(StorageError::NotFound(format!("run {run_id}")));
        }
        self.safe_run_output_paths(run_id).await.map(|_| ())
    }

    pub async fn delete_run(
        &self,
        run_id: &str,
        delete_local_assets: bool,
    ) -> StorageResult<RunDeleteResult> {
        if self.run(run_id).await?.is_none() {
            return Ok(RunDeleteResult::default());
        }
        let paths = if delete_local_assets {
            self.safe_run_output_paths(run_id).await?
        } else {
            Vec::new()
        };
        let mut local_assets_deleted = 0_u32;
        for path in paths {
            if path.is_file() {
                fs::remove_file(&path)?;
                local_assets_deleted = local_assets_deleted.saturating_add(1);
                prune_empty_output_directories(&self.layout, &path)?;
            }
        }
        let result = sqlx::query("DELETE FROM runs WHERE id = ?")
            .bind(run_id)
            .execute(&self.pool)
            .await?;
        Ok(RunDeleteResult {
            deleted: result.rows_affected() > 0,
            local_assets_deleted,
        })
    }

    pub async fn set_run_status(&self, id: &str, status: RunStatus) -> StorageResult<()> {
        let mut run = self
            .run(id)
            .await?
            .ok_or_else(|| StorageError::NotFound(format!("run {id}")))?;
        run.status = status;
        run.updated_at = Utc::now();
        self.upsert_run(&run).await
    }

    pub async fn upsert_job(&self, job: &JobRecord) -> StorageResult<()> {
        sqlx::query(
            r#"INSERT INTO jobs
               (id, run_id, sequence, status, remote_job_id, remote_batch_id, job_json, created_at, updated_at)
               VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?)
               ON CONFLICT(id) DO UPDATE SET
                 status = excluded.status,
                 remote_job_id = excluded.remote_job_id,
                 remote_batch_id = excluded.remote_batch_id,
                 job_json = excluded.job_json,
                 updated_at = excluded.updated_at"#,
        )
        .bind(&job.id)
        .bind(&job.run_id)
        .bind(job.sequence)
        .bind(enum_key(&job.status)?)
        .bind(&job.remote_job_id)
        .bind(&job.remote_batch_id)
        .bind(serde_json::to_string(job)?)
        .bind(job.created_at.to_rfc3339())
        .bind(job.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn job(&self, id: &str) -> StorageResult<Option<JobRecord>> {
        fetch_json_by_id(&self.pool, "jobs", "job_json", id).await
    }

    pub async fn list_jobs(&self, run_id: &str) -> StorageResult<Vec<JobRecord>> {
        let rows = sqlx::query("SELECT job_json FROM jobs WHERE run_id = ? ORDER BY sequence")
            .bind(run_id)
            .fetch_all(&self.pool)
            .await?;
        deserialize_rows(rows, "job_json")
    }

    pub async fn set_job_status(&self, id: &str, status: JobStatus) -> StorageResult<()> {
        let mut job = self
            .job(id)
            .await?
            .ok_or_else(|| StorageError::NotFound(format!("job {id}")))?;
        job.status = status;
        job.updated_at = Utc::now();
        self.upsert_job(&job).await
    }

    pub async fn save_input_asset(
        &self,
        asset: &InputAsset,
        run_id: Option<&str>,
        job_id: Option<&str>,
    ) -> StorageResult<()> {
        sqlx::query(
            r#"INSERT INTO input_assets(id, run_id, job_id, asset_json, created_at) VALUES(?, ?, ?, ?, ?)
               ON CONFLICT(id) DO UPDATE SET run_id = excluded.run_id, job_id = excluded.job_id, asset_json = excluded.asset_json"#,
        )
        .bind(&asset.id)
        .bind(run_id)
        .bind(job_id)
        .bind(serde_json::to_string(asset)?)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_input_assets(&self, run_id: &str) -> StorageResult<Vec<InputAsset>> {
        let rows = sqlx::query(
            "SELECT asset_json FROM input_assets WHERE run_id = ? ORDER BY created_at, id",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;
        deserialize_rows(rows, "asset_json")
    }

    pub async fn upsert_output(&self, output: &OutputPart) -> StorageResult<()> {
        let local_path = output
            .local_path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned());
        sqlx::query(
            r#"INSERT INTO output_parts
               (id, run_id, job_id, sequence, kind, local_path, output_json, created_at)
               VALUES(?, ?, ?, ?, ?, ?, ?, ?)
               ON CONFLICT(id) DO UPDATE SET
                 job_id = excluded.job_id,
                 sequence = excluded.sequence,
                 kind = excluded.kind,
                 local_path = excluded.local_path,
                 output_json = excluded.output_json"#,
        )
        .bind(&output.id)
        .bind(&output.run_id)
        .bind(&output.job_id)
        .bind(output.sequence)
        .bind(enum_key(&output.kind)?)
        .bind(local_path)
        .bind(serde_json::to_string(output)?)
        .bind(output.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_outputs(&self, run_id: &str) -> StorageResult<Vec<OutputPart>> {
        let rows = sqlx::query(
            "SELECT output_json FROM output_parts WHERE run_id = ? ORDER BY sequence, id",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;
        deserialize_rows(rows, "output_json")
    }

    pub async fn save_usage(&self, usage: &UsageRecord) -> StorageResult<()> {
        sqlx::query(
            r#"INSERT INTO usage_records(id, run_id, job_id, usage_json, created_at) VALUES(?, ?, ?, ?, ?)
               ON CONFLICT(id) DO UPDATE SET usage_json = excluded.usage_json"#,
        )
        .bind(&usage.id)
        .bind(&usage.run_id)
        .bind(&usage.job_id)
        .bind(serde_json::to_string(usage)?)
        .bind(usage.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_usage(&self, run_id: &str) -> StorageResult<Vec<UsageRecord>> {
        let rows = sqlx::query(
            "SELECT usage_json FROM usage_records WHERE run_id = ? ORDER BY created_at",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;
        deserialize_rows(rows, "usage_json")
    }

    pub async fn save_error(&self, error: &ErrorRecord) -> StorageResult<()> {
        sqlx::query(
            r#"INSERT INTO errors(id, run_id, job_id, code, retryable, error_json, created_at)
               VALUES(?, ?, ?, ?, ?, ?, ?)
               ON CONFLICT(id) DO UPDATE SET error_json = excluded.error_json"#,
        )
        .bind(&error.id)
        .bind(&error.run_id)
        .bind(&error.job_id)
        .bind(&error.error.code)
        .bind(error.error.retryable)
        .bind(serde_json::to_string(error)?)
        .bind(error.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_errors(&self, run_id: &str) -> StorageResult<Vec<ErrorRecord>> {
        let rows =
            sqlx::query("SELECT error_json FROM errors WHERE run_id = ? ORDER BY created_at")
                .bind(run_id)
                .fetch_all(&self.pool)
                .await?;
        deserialize_rows(rows, "error_json")
    }

    pub async fn upsert_remote_task(&self, task: &RemoteTaskRecord) -> StorageResult<()> {
        sqlx::query(
            r#"INSERT INTO remote_tasks
               (id, run_id, job_id, provider_profile_id, remote_id, task_type, status, next_poll_at, task_json, created_at, updated_at)
               VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
               ON CONFLICT(id) DO UPDATE SET
                 status = excluded.status,
                 next_poll_at = excluded.next_poll_at,
                 task_json = excluded.task_json,
                 updated_at = excluded.updated_at"#,
        )
        .bind(&task.id)
        .bind(&task.run_id)
        .bind(&task.job_id)
        .bind(&task.provider_profile_id)
        .bind(&task.remote_id)
        .bind(&task.task_type)
        .bind(&task.status)
        .bind(task.next_poll_at.map(|value| value.to_rfc3339()))
        .bind(serde_json::to_string(task)?)
        .bind(task.created_at.to_rfc3339())
        .bind(task.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn due_remote_tasks(
        &self,
        now: DateTime<Utc>,
    ) -> StorageResult<Vec<RemoteTaskRecord>> {
        let rows = sqlx::query(
            r#"SELECT task_json FROM remote_tasks
               WHERE status NOT IN ('succeeded', 'failed', 'cancelled')
                 AND (next_poll_at IS NULL OR next_poll_at <= ?)
               ORDER BY COALESCE(next_poll_at, created_at), id"#,
        )
        .bind(now.to_rfc3339())
        .fetch_all(&self.pool)
        .await?;
        deserialize_rows(rows, "task_json")
    }

    pub async fn remote_tasks_for_run(&self, run_id: &str) -> StorageResult<Vec<RemoteTaskRecord>> {
        let rows = sqlx::query(
            "SELECT task_json FROM remote_tasks WHERE run_id = ? ORDER BY created_at, id",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;
        deserialize_rows(rows, "task_json")
    }

    pub async fn update_remote_task_status(
        &self,
        id: &str,
        status: &str,
        next_poll_at: Option<DateTime<Utc>>,
        metadata: Option<BTreeMap<String, Value>>,
    ) -> StorageResult<bool> {
        let json: Option<String> =
            sqlx::query_scalar("SELECT task_json FROM remote_tasks WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        let Some(mut task) = deserialize_optional::<RemoteTaskRecord>(json)? else {
            return Ok(false);
        };
        task.status = status.to_owned();
        task.next_poll_at = next_poll_at;
        if let Some(metadata) = metadata {
            task.metadata = metadata;
        }
        task.updated_at = Utc::now();
        self.upsert_remote_task(&task).await?;
        Ok(true)
    }

    pub async fn delete_remote_task(&self, id: &str) -> StorageResult<bool> {
        let result = sqlx::query("DELETE FROM remote_tasks WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn upsert_remote_file(&self, file: &RemoteFileRecord) -> StorageResult<()> {
        sqlx::query(
            r#"INSERT INTO remote_files
               (id, provider_profile_id, provider_file_id, purpose, expires_at, file_json, created_at)
               VALUES(?, ?, ?, ?, ?, ?, ?)
               ON CONFLICT(id) DO UPDATE SET
                 purpose = excluded.purpose,
                 expires_at = excluded.expires_at,
                 file_json = excluded.file_json"#,
        )
        .bind(&file.id)
        .bind(&file.provider_profile_id)
        .bind(&file.provider_file_id)
        .bind(&file.purpose)
        .bind(file.expires_at.map(|value| value.to_rfc3339()))
        .bind(serde_json::to_string(file)?)
        .bind(file.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_remote_files(
        &self,
        provider_profile_id: &str,
    ) -> StorageResult<Vec<RemoteFileRecord>> {
        let rows = sqlx::query(
            "SELECT file_json FROM remote_files WHERE provider_profile_id = ? ORDER BY created_at DESC",
        )
        .bind(provider_profile_id)
        .fetch_all(&self.pool)
        .await?;
        deserialize_rows(rows, "file_json")
    }

    pub async fn remote_file(
        &self,
        provider_profile_id: &str,
        provider_file_id: &str,
    ) -> StorageResult<Option<RemoteFileRecord>> {
        let json: Option<String> = sqlx::query_scalar(
            "SELECT file_json FROM remote_files WHERE provider_profile_id = ? AND provider_file_id = ?",
        )
        .bind(provider_profile_id)
        .bind(provider_file_id)
        .fetch_optional(&self.pool)
        .await?;
        deserialize_optional(json)
    }

    pub async fn remote_files_for_run(&self, run_id: &str) -> StorageResult<Vec<RemoteFileRecord>> {
        let Some(run) = self.run(run_id).await? else {
            return Ok(Vec::new());
        };
        let file_ids = self
            .list_outputs(run_id)
            .await?
            .into_iter()
            .filter_map(|output| output.provider_file_id)
            .collect::<HashSet<_>>();
        if file_ids.is_empty() {
            return Ok(Vec::new());
        }
        Ok(self
            .list_remote_files(&run.request.provider_profile_id)
            .await?
            .into_iter()
            .filter(|file| file_ids.contains(&file.provider_file_id))
            .collect())
    }

    pub async fn remote_file_is_referenced_elsewhere(
        &self,
        run_id: &str,
        provider_profile_id: &str,
        provider_file_id: &str,
    ) -> StorageResult<bool> {
        let rows = sqlx::query(
            r#"SELECT output_parts.output_json
               FROM output_parts
               INNER JOIN runs ON runs.id = output_parts.run_id
               WHERE output_parts.run_id <> ? AND runs.provider_profile_id = ?"#,
        )
        .bind(run_id)
        .bind(provider_profile_id)
        .fetch_all(&self.pool)
        .await?;
        for row in rows {
            let output: OutputPart =
                serde_json::from_str(row.get::<String, _>("output_json").as_str())?;
            if output.provider_file_id.as_deref() == Some(provider_file_id) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub async fn delete_remote_file(&self, id: &str) -> StorageResult<bool> {
        let result = sqlx::query("DELETE FROM remote_files WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn record_remote_file_delete_error(
        &self,
        id: &str,
        error: Value,
    ) -> StorageResult<bool> {
        let json: Option<String> =
            sqlx::query_scalar("SELECT file_json FROM remote_files WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        let Some(mut file) = deserialize_optional::<RemoteFileRecord>(json)? else {
            return Ok(false);
        };
        file.metadata.insert("lastDeleteError".to_owned(), error);
        file.metadata.insert(
            "lastDeleteAttemptAt".to_owned(),
            Value::String(Utc::now().to_rfc3339()),
        );
        self.upsert_remote_file(&file).await?;
        Ok(true)
    }

    pub async fn remap_provider(
        &self,
        from_provider_id: &str,
        to_provider_id: &str,
    ) -> StorageResult<ProviderRemapResult> {
        if from_provider_id.trim().is_empty() || to_provider_id.trim().is_empty() {
            return Err(StorageError::InvalidProject(
                "provider IDs are required for remapping".to_owned(),
            ));
        }
        if from_provider_id == to_provider_id {
            return Ok(ProviderRemapResult::default());
        }
        let mut transaction = self.pool.begin().await?;
        let mut result = ProviderRemapResult::default();

        let summary_json: Option<String> =
            sqlx::query_scalar("SELECT summary_json FROM project_metadata WHERE singleton = 1")
                .fetch_optional(&mut *transaction)
                .await?;
        if let Some(summary_json) = summary_json {
            let mut summary: ProjectSummary = serde_json::from_str(&summary_json)?;
            if summary.default_provider_profile_id.as_deref() == Some(from_provider_id) {
                summary.default_provider_profile_id = Some(to_provider_id.to_owned());
                summary.updated_at = Utc::now();
                sqlx::query(
                    "UPDATE project_metadata SET summary_json = ?, updated_at = ? WHERE singleton = 1",
                )
                .bind(serde_json::to_string(&summary)?)
                .bind(summary.updated_at.to_rfc3339())
                .execute(&mut *transaction)
                .await?;
                result.summaries = 1;
            }
        }

        let preset_rows = sqlx::query("SELECT id, preset_json FROM generation_presets")
            .fetch_all(&mut *transaction)
            .await?;
        for row in preset_rows {
            let mut preset: GenerationPreset =
                serde_json::from_str(row.get::<String, _>("preset_json").as_str())?;
            if preset.provider_profile_id.as_deref() != Some(from_provider_id) {
                continue;
            }
            preset.provider_profile_id = Some(to_provider_id.to_owned());
            preset.updated_at = Utc::now();
            sqlx::query(
                "UPDATE generation_presets SET preset_json = ?, updated_at = ? WHERE id = ?",
            )
            .bind(serde_json::to_string(&preset)?)
            .bind(preset.updated_at.to_rfc3339())
            .bind(row.get::<String, _>("id"))
            .execute(&mut *transaction)
            .await?;
            result.presets = result.presets.saturating_add(1);
        }

        let run_rows = sqlx::query("SELECT id, run_json FROM runs WHERE provider_profile_id = ?")
            .bind(from_provider_id)
            .fetch_all(&mut *transaction)
            .await?;
        for row in run_rows {
            let mut run: RunRecord =
                serde_json::from_str(row.get::<String, _>("run_json").as_str())?;
            run.request.provider_profile_id = to_provider_id.to_owned();
            run.updated_at = Utc::now();
            sqlx::query(
                "UPDATE runs SET provider_profile_id = ?, run_json = ?, updated_at = ? WHERE id = ?",
            )
            .bind(to_provider_id)
            .bind(serde_json::to_string(&run)?)
            .bind(run.updated_at.to_rfc3339())
            .bind(row.get::<String, _>("id"))
            .execute(&mut *transaction)
            .await?;
            result.runs = result.runs.saturating_add(1);
        }

        let job_rows = sqlx::query("SELECT id, job_json FROM jobs")
            .fetch_all(&mut *transaction)
            .await?;
        for row in job_rows {
            let mut job: JobRecord =
                serde_json::from_str(row.get::<String, _>("job_json").as_str())?;
            if job.request.provider_profile_id != from_provider_id {
                continue;
            }
            job.request.provider_profile_id = to_provider_id.to_owned();
            job.updated_at = Utc::now();
            sqlx::query("UPDATE jobs SET job_json = ?, updated_at = ? WHERE id = ?")
                .bind(serde_json::to_string(&job)?)
                .bind(job.updated_at.to_rfc3339())
                .bind(row.get::<String, _>("id"))
                .execute(&mut *transaction)
                .await?;
            result.jobs = result.jobs.saturating_add(1);
        }

        let conversation_result = sqlx::query(
            "UPDATE conversations SET provider_profile_id = ?, updated_at = ? WHERE provider_profile_id = ?",
        )
        .bind(to_provider_id)
        .bind(Utc::now().to_rfc3339())
        .bind(from_provider_id)
        .execute(&mut *transaction)
        .await?;
        result.conversations = conversation_result.rows_affected() as u32;

        let task_rows =
            sqlx::query("SELECT id, task_json FROM remote_tasks WHERE provider_profile_id = ?")
                .bind(from_provider_id)
                .fetch_all(&mut *transaction)
                .await?;
        for row in task_rows {
            let mut task: RemoteTaskRecord =
                serde_json::from_str(row.get::<String, _>("task_json").as_str())?;
            task.provider_profile_id = to_provider_id.to_owned();
            task.updated_at = Utc::now();
            sqlx::query(
                "UPDATE remote_tasks SET provider_profile_id = ?, task_json = ?, updated_at = ? WHERE id = ?",
            )
            .bind(to_provider_id)
            .bind(serde_json::to_string(&task)?)
            .bind(task.updated_at.to_rfc3339())
            .bind(row.get::<String, _>("id"))
            .execute(&mut *transaction)
            .await?;
            result.remote_tasks = result.remote_tasks.saturating_add(1);
        }

        let file_rows =
            sqlx::query("SELECT id, file_json FROM remote_files WHERE provider_profile_id = ?")
                .bind(from_provider_id)
                .fetch_all(&mut *transaction)
                .await?;
        for row in file_rows {
            let mut file: RemoteFileRecord =
                serde_json::from_str(row.get::<String, _>("file_json").as_str())?;
            file.provider_profile_id = to_provider_id.to_owned();
            sqlx::query(
                "UPDATE remote_files SET provider_profile_id = ?, file_json = ? WHERE id = ?",
            )
            .bind(to_provider_id)
            .bind(serde_json::to_string(&file)?)
            .bind(row.get::<String, _>("id"))
            .execute(&mut *transaction)
            .await?;
            result.remote_files = result.remote_files.saturating_add(1);
        }

        transaction.commit().await?;
        Ok(result)
    }

    async fn safe_run_output_paths(&self, run_id: &str) -> StorageResult<Vec<PathBuf>> {
        let output_directory = self.layout.output_directory();
        let output_root = strip_extended_length_prefix(output_directory.canonicalize()?);
        let mut paths = HashSet::new();
        for output in self.list_outputs(run_id).await? {
            let Some(relative) = output.local_path else {
                continue;
            };
            let resolved = self.layout.resolve_relative(&relative)?;
            if !resolved.starts_with(&output_directory) {
                return Err(StorageError::InvalidPath(relative));
            }
            if resolved.exists() {
                let canonical = strip_extended_length_prefix(resolved.canonicalize()?);
                if !canonical.starts_with(&output_root) {
                    return Err(StorageError::InvalidPath(relative));
                }
                paths.insert(canonical);
            }
        }
        Ok(paths.into_iter().collect())
    }

    /// Marks local in-flight work as interrupted. Provider background and batch
    /// tasks remain in `remote_tasks`, so the scheduler can resume polling them.
    pub async fn mark_local_work_interrupted(&self) -> StorageResult<()> {
        let run_rows = sqlx::query(
            "SELECT run_json FROM runs WHERE status IN ('queued', 'running', 'paused')",
        )
        .fetch_all(&self.pool)
        .await?;
        for row in run_rows {
            let mut run: RunRecord =
                serde_json::from_str(row.get::<String, _>("run_json").as_str())?;
            run.status = RunStatus::Interrupted;
            run.updated_at = Utc::now();
            self.upsert_run(&run).await?;
        }
        let job_rows = sqlx::query(
            "SELECT job_json FROM jobs WHERE status IN ('queued', 'submitting', 'running')",
        )
        .fetch_all(&self.pool)
        .await?;
        for row in job_rows {
            let mut job: JobRecord =
                serde_json::from_str(row.get::<String, _>("job_json").as_str())?;
            job.status = JobStatus::Interrupted;
            job.updated_at = Utc::now();
            self.upsert_job(&job).await?;
        }
        Ok(())
    }
}

fn prune_empty_output_directories(layout: &ProjectLayout, path: &Path) -> StorageResult<()> {
    let output_root = strip_extended_length_prefix(layout.output_directory().canonicalize()?);
    let mut parent = path.parent().map(Path::to_owned);
    while let Some(directory) = parent {
        if directory == output_root || !directory.starts_with(&output_root) {
            break;
        }
        match fs::remove_dir(&directory) {
            Ok(()) => parent = directory.parent().map(Path::to_owned),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::DirectoryNotEmpty | std::io::ErrorKind::NotFound
                ) =>
            {
                break;
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

async fn connect(layout: &ProjectLayout) -> StorageResult<SqlitePool> {
    let options = SqliteConnectOptions::new()
        .filename(layout.database_path())
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal);
    Ok(SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?)
}

async fn fetch_json_by_id<T: DeserializeOwned>(
    pool: &SqlitePool,
    table: &str,
    column: &str,
    id: &str,
) -> StorageResult<Option<T>> {
    let allowed = [("runs", "run_json"), ("jobs", "job_json")];
    if !allowed.contains(&(table, column)) {
        return Err(StorageError::InvalidProject(
            "invalid repository table selection".to_owned(),
        ));
    }
    let sql = format!("SELECT {column} FROM {table} WHERE id = ?");
    let json: Option<String> = sqlx::query_scalar(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await?;
    deserialize_optional(json)
}

async fn fetch_json_list<T: DeserializeOwned>(
    pool: &SqlitePool,
    sql: &str,
    column: &str,
) -> StorageResult<Vec<T>> {
    let rows = sqlx::query(sql).fetch_all(pool).await?;
    deserialize_rows(rows, column)
}

fn deserialize_rows<T: DeserializeOwned>(
    rows: Vec<sqlx::sqlite::SqliteRow>,
    column: &str,
) -> StorageResult<Vec<T>> {
    rows.into_iter()
        .map(|row| serde_json::from_str(row.get::<String, _>(column).as_str()).map_err(Into::into))
        .collect()
}

fn deserialize_optional<T: DeserializeOwned>(json: Option<String>) -> StorageResult<Option<T>> {
    json.map(|json| serde_json::from_str(&json).map_err(Into::into))
        .transpose()
}

fn deserialize_required<T: DeserializeOwned>(
    json: Option<String>,
    entity: &str,
) -> StorageResult<T> {
    deserialize_optional(json)?.ok_or_else(|| StorageError::NotFound(entity.to_owned()))
}

async fn delete_by_id(pool: &SqlitePool, table: &str, id: &str) -> StorageResult<bool> {
    if !matches!(table, "prompt_contexts" | "generation_presets") {
        return Err(StorageError::InvalidProject(
            "invalid repository table selection".to_owned(),
        ));
    }
    let sql = format!("DELETE FROM {table} WHERE id = ?");
    let result = sqlx::query(&sql).bind(id).execute(pool).await?;
    Ok(result.rows_affected() > 0)
}

fn enum_key<T: Serialize>(value: &T) -> StorageResult<String> {
    match serde_json::to_value(value)? {
        Value::String(value) => Ok(value),
        _ => Err(StorageError::InvalidProject(
            "expected a string enum representation".to_owned(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::domain::{
        ExecutionMode, GenerationRequest, ModelCapability, Operation, OutputPartKind, ProviderKind,
    };

    use super::*;

    fn project_root(name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let root = directory
            .path()
            .join(format!("imageworkbench-project-{name}-{}", Uuid::new_v4()));
        (directory, root)
    }

    fn run_record(provider_profile_id: &str, run_id: &str) -> RunRecord {
        let mut capability = ModelCapability::generic(ProviderKind::OpenAi, "gpt-image-1");
        capability.operations = BTreeSet::from([Operation::Generate]);
        capability.execution_modes = BTreeSet::from([ExecutionMode::Realtime]);
        let now = Utc::now();
        RunRecord {
            id: run_id.to_owned(),
            group_id: None,
            request: GenerationRequest::new("project", provider_profile_id, "gpt-image-1", "draw"),
            status: RunStatus::Queued,
            raw_prompt: "draw".to_owned(),
            final_prompt: "draw".to_owned(),
            context_snapshot: Vec::new(),
            preset_snapshot: None,
            capability_snapshot: capability,
            capability_registry_version: "2026-07-12".to_owned(),
            model_version: None,
            provider_request_id: None,
            redacted_request: None,
            redacted_response: None,
            started_at: None,
            finished_at: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[tokio::test]
    async fn creates_and_reopens_a_portable_project() {
        let (_directory, root) = project_root("portable");
        let store = ProjectStore::create(&root, "Portable").await.unwrap();
        let id = store.summary().await.unwrap().id;
        store.pool.close().await;
        drop(store);

        let reopened = ProjectStore::open(&root).await.unwrap();
        let summary = reopened.summary().await.unwrap();
        assert_eq!(summary.id, id);
        assert_eq!(
            summary.root_path,
            strip_extended_length_prefix(root.canonicalize().unwrap())
        );

        reopened.pool.close().await;
        drop(reopened);
    }

    #[tokio::test]
    async fn persists_contexts_and_full_run_snapshots() {
        let (_directory, root) = project_root("records");
        let store = ProjectStore::create(&root, "Records").await.unwrap();
        let context = PromptContext::new("Style", "anime");
        store.upsert_prompt_context(&context).await.unwrap();

        let mut capability = ModelCapability::generic(ProviderKind::OpenAi, "gpt-image-1");
        capability.operations = BTreeSet::from([Operation::Generate]);
        capability.execution_modes = BTreeSet::from([ExecutionMode::Realtime]);
        let request = GenerationRequest::new("project", "provider", "gpt-image-1", "draw");
        let now = Utc::now();
        let run = RunRecord {
            id: Uuid::new_v4().to_string(),
            group_id: None,
            request,
            status: RunStatus::Queued,
            raw_prompt: "draw".to_owned(),
            final_prompt: "anime\n\ndraw".to_owned(),
            context_snapshot: vec![context.clone()],
            preset_snapshot: None,
            capability_snapshot: capability,
            capability_registry_version: "2026-07-12".to_owned(),
            model_version: None,
            provider_request_id: None,
            redacted_request: None,
            redacted_response: None,
            started_at: None,
            finished_at: None,
            created_at: now,
            updated_at: now,
        };
        store.upsert_run(&run).await.unwrap();

        assert_eq!(store.list_prompt_contexts().await.unwrap(), vec![context]);
        assert_eq!(store.run(&run.id).await.unwrap(), Some(run));

        store.pool.close().await;
        drop(store);
    }

    #[tokio::test]
    async fn remaps_provider_references_in_portable_project_records() {
        let (_directory, root) = project_root("remap");
        let store = ProjectStore::create(&root, "Remap").await.unwrap();
        let mut summary = store.summary().await.unwrap();
        summary.default_provider_profile_id = Some("provider-old".to_owned());
        store.save_summary(&summary).await.unwrap();
        let now = Utc::now();
        store
            .upsert_preset(&GenerationPreset {
                id: "preset".to_owned(),
                name: "Preset".to_owned(),
                provider_profile_id: Some("provider-old".to_owned()),
                model_id: Some("gpt-image-1".to_owned()),
                operation: Some(Operation::Generate),
                parameters: BTreeMap::new(),
                output: None,
                created_at: now,
                updated_at: now,
            })
            .await
            .unwrap();
        let run = run_record("provider-old", "run-remap");
        store.upsert_run(&run).await.unwrap();
        store
            .upsert_job(&JobRecord {
                id: "job-remap".to_owned(),
                run_id: run.id.clone(),
                sequence: 0,
                status: JobStatus::WaitingRemote,
                attempt: 1,
                remote_job_id: Some("remote-job".to_owned()),
                remote_batch_id: None,
                next_poll_at: Some(now),
                request: run.request.clone(),
                created_at: now,
                updated_at: now,
            })
            .await
            .unwrap();
        store
            .upsert_remote_task(&RemoteTaskRecord {
                id: "task-remap".to_owned(),
                run_id: run.id.clone(),
                job_id: Some("job-remap".to_owned()),
                provider_profile_id: "provider-old".to_owned(),
                remote_id: "remote-job".to_owned(),
                task_type: "background".to_owned(),
                status: "running".to_owned(),
                next_poll_at: Some(now),
                metadata: BTreeMap::new(),
                created_at: now,
                updated_at: now,
            })
            .await
            .unwrap();
        store
            .upsert_remote_file(&RemoteFileRecord {
                id: "file-remap".to_owned(),
                provider_profile_id: "provider-old".to_owned(),
                provider_file_id: "file-123".to_owned(),
                purpose: Some("output.png".to_owned()),
                expires_at: None,
                metadata: BTreeMap::new(),
                created_at: now,
            })
            .await
            .unwrap();

        let result = store
            .remap_provider("provider-old", "provider-new")
            .await
            .unwrap();

        assert_eq!(result.summaries, 1);
        assert_eq!(result.presets, 1);
        assert_eq!(result.runs, 1);
        assert_eq!(result.jobs, 1);
        assert_eq!(result.remote_tasks, 1);
        assert_eq!(result.remote_files, 1);
        assert_eq!(
            store
                .summary()
                .await
                .unwrap()
                .default_provider_profile_id
                .as_deref(),
            Some("provider-new")
        );
        assert_eq!(
            store
                .run(&run.id)
                .await
                .unwrap()
                .unwrap()
                .request
                .provider_profile_id,
            "provider-new"
        );
        assert_eq!(
            store
                .job("job-remap")
                .await
                .unwrap()
                .unwrap()
                .request
                .provider_profile_id,
            "provider-new"
        );
        assert_eq!(
            store.remote_tasks_for_run(&run.id).await.unwrap()[0].provider_profile_id,
            "provider-new"
        );
        assert_eq!(
            store.list_remote_files("provider-new").await.unwrap().len(),
            1
        );
        assert!(
            store
                .list_remote_files("provider-old")
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn deletes_run_cascade_and_only_relative_project_outputs() {
        let (_directory, root) = project_root("delete-run");
        let store = ProjectStore::create(&root, "Delete run").await.unwrap();
        let run = run_record("provider", "run-delete");
        store.upsert_run(&run).await.unwrap();
        let now = Utc::now();
        let output_directory = store
            .layout()
            .output_run_directory(now.date_naive(), &run.id)
            .unwrap();
        let output_path = output_directory.join("001.png");
        fs::write(&output_path, b"image").unwrap();
        let relative = output_path
            .strip_prefix(store.layout().root())
            .unwrap()
            .to_owned();
        store
            .upsert_output(&OutputPart {
                id: "output-delete".to_owned(),
                run_id: run.id.clone(),
                job_id: None,
                sequence: 0,
                kind: OutputPartKind::Image,
                text: None,
                local_path: Some(relative),
                remote_url: None,
                provider_file_id: Some("file-delete".to_owned()),
                mime_type: Some("image/png".to_owned()),
                sha256: None,
                size_bytes: Some(5),
                metadata: BTreeMap::new(),
                created_at: now,
            })
            .await
            .unwrap();
        store
            .upsert_remote_task(&RemoteTaskRecord {
                id: "task-delete".to_owned(),
                run_id: run.id.clone(),
                job_id: None,
                provider_profile_id: "provider".to_owned(),
                remote_id: "remote-delete".to_owned(),
                task_type: "background".to_owned(),
                status: "succeeded".to_owned(),
                next_poll_at: None,
                metadata: BTreeMap::new(),
                created_at: now,
                updated_at: now,
            })
            .await
            .unwrap();
        store
            .upsert_remote_file(&RemoteFileRecord {
                id: "remote-file-delete".to_owned(),
                provider_profile_id: "provider".to_owned(),
                provider_file_id: "file-delete".to_owned(),
                purpose: None,
                expires_at: None,
                metadata: BTreeMap::new(),
                created_at: now,
            })
            .await
            .unwrap();

        let result = store.delete_run(&run.id, true).await.unwrap();

        assert!(result.deleted);
        assert_eq!(result.local_assets_deleted, 1);
        assert!(!output_path.exists());
        assert!(store.run(&run.id).await.unwrap().is_none());
        assert!(
            store
                .remote_tasks_for_run(&run.id)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(store.list_remote_files("provider").await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn refuses_to_delete_output_paths_outside_project() {
        let (directory, root) = project_root("delete-outside");
        let store = ProjectStore::create(&root, "Delete outside").await.unwrap();
        let run = run_record("provider", "run-outside");
        store.upsert_run(&run).await.unwrap();
        let outside = directory.path().join("outside.png");
        fs::write(&outside, b"do not delete").unwrap();
        store
            .upsert_output(&OutputPart {
                id: "output-outside".to_owned(),
                run_id: run.id.clone(),
                job_id: None,
                sequence: 0,
                kind: OutputPartKind::Image,
                text: None,
                local_path: Some(outside.clone()),
                remote_url: None,
                provider_file_id: None,
                mime_type: Some("image/png".to_owned()),
                sha256: None,
                size_bytes: Some(13),
                metadata: BTreeMap::new(),
                created_at: Utc::now(),
            })
            .await
            .unwrap();

        assert!(store.delete_run(&run.id, true).await.is_err());
        assert!(outside.exists());
        assert!(store.run(&run.id).await.unwrap().is_some());
    }
}
