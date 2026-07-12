use sqlx::{Sqlite, SqlitePool, Transaction};

use super::StorageResult;

pub(crate) const GLOBAL_MIGRATIONS: &[&str] = &[r#"
    CREATE TABLE provider_profiles (
        id TEXT PRIMARY KEY NOT NULL,
        name TEXT NOT NULL,
        kind TEXT NOT NULL,
        credential_ref TEXT,
        profile_json TEXT NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    );
    CREATE INDEX provider_profiles_kind_idx ON provider_profiles(kind);
    CREATE TABLE app_settings (
        key TEXT PRIMARY KEY NOT NULL,
        value_json TEXT NOT NULL,
        updated_at TEXT NOT NULL
    );
    CREATE TABLE recent_projects (
        project_id TEXT PRIMARY KEY NOT NULL,
        root_path TEXT NOT NULL UNIQUE,
        name TEXT NOT NULL,
        summary_json TEXT NOT NULL,
        last_opened_at TEXT NOT NULL
    );
    CREATE INDEX recent_projects_opened_idx ON recent_projects(last_opened_at DESC);
    CREATE TABLE model_capability_overrides (
        provider_profile_id TEXT NOT NULL,
        model_id TEXT NOT NULL,
        patch_json TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        PRIMARY KEY(provider_profile_id, model_id),
        FOREIGN KEY(provider_profile_id) REFERENCES provider_profiles(id) ON DELETE CASCADE
    );
    "#];

pub(crate) const PROJECT_MIGRATIONS: &[&str] = &[r#"
    CREATE TABLE project_metadata (
        singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton = 1),
        summary_json TEXT NOT NULL,
        updated_at TEXT NOT NULL
    );
    CREATE TABLE prompt_contexts (
        id TEXT PRIMARY KEY NOT NULL,
        name TEXT NOT NULL,
        enabled INTEGER NOT NULL,
        sort_order INTEGER NOT NULL,
        context_json TEXT NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    );
    CREATE INDEX prompt_contexts_order_idx ON prompt_contexts(enabled DESC, sort_order, id);
    CREATE TABLE generation_presets (
        id TEXT PRIMARY KEY NOT NULL,
        name TEXT NOT NULL,
        preset_json TEXT NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    );
    CREATE TABLE conversations (
        id TEXT PRIMARY KEY NOT NULL,
        title TEXT NOT NULL,
        provider_profile_id TEXT,
        model_id TEXT,
        state_json TEXT NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    );
    CREATE TABLE run_groups (
        id TEXT PRIMARY KEY NOT NULL,
        name TEXT,
        metadata_json TEXT NOT NULL,
        created_at TEXT NOT NULL
    );
    CREATE TABLE runs (
        id TEXT PRIMARY KEY NOT NULL,
        group_id TEXT,
        provider_profile_id TEXT NOT NULL,
        model_id TEXT NOT NULL,
        operation TEXT NOT NULL,
        execution_mode TEXT NOT NULL,
        status TEXT NOT NULL,
        run_json TEXT NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        FOREIGN KEY(group_id) REFERENCES run_groups(id) ON DELETE SET NULL
    );
    CREATE INDEX runs_created_idx ON runs(created_at DESC);
    CREATE INDEX runs_filter_idx ON runs(provider_profile_id, model_id, status);
    CREATE TABLE jobs (
        id TEXT PRIMARY KEY NOT NULL,
        run_id TEXT NOT NULL,
        sequence INTEGER NOT NULL,
        status TEXT NOT NULL,
        remote_job_id TEXT,
        remote_batch_id TEXT,
        job_json TEXT NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        FOREIGN KEY(run_id) REFERENCES runs(id) ON DELETE CASCADE
    );
    CREATE UNIQUE INDEX jobs_run_sequence_idx ON jobs(run_id, sequence);
    CREATE INDEX jobs_remote_idx ON jobs(remote_job_id, remote_batch_id);
    CREATE TABLE input_assets (
        id TEXT PRIMARY KEY NOT NULL,
        run_id TEXT,
        job_id TEXT,
        asset_json TEXT NOT NULL,
        created_at TEXT NOT NULL,
        FOREIGN KEY(run_id) REFERENCES runs(id) ON DELETE CASCADE,
        FOREIGN KEY(job_id) REFERENCES jobs(id) ON DELETE CASCADE
    );
    CREATE TABLE output_parts (
        id TEXT PRIMARY KEY NOT NULL,
        run_id TEXT NOT NULL,
        job_id TEXT,
        sequence INTEGER NOT NULL,
        kind TEXT NOT NULL,
        local_path TEXT,
        output_json TEXT NOT NULL,
        created_at TEXT NOT NULL,
        FOREIGN KEY(run_id) REFERENCES runs(id) ON DELETE CASCADE,
        FOREIGN KEY(job_id) REFERENCES jobs(id) ON DELETE SET NULL
    );
    CREATE INDEX output_parts_run_idx ON output_parts(run_id, sequence);
    CREATE TABLE usage_records (
        id TEXT PRIMARY KEY NOT NULL,
        run_id TEXT NOT NULL,
        job_id TEXT,
        usage_json TEXT NOT NULL,
        created_at TEXT NOT NULL,
        FOREIGN KEY(run_id) REFERENCES runs(id) ON DELETE CASCADE,
        FOREIGN KEY(job_id) REFERENCES jobs(id) ON DELETE SET NULL
    );
    CREATE TABLE errors (
        id TEXT PRIMARY KEY NOT NULL,
        run_id TEXT NOT NULL,
        job_id TEXT,
        code TEXT NOT NULL,
        retryable INTEGER NOT NULL,
        error_json TEXT NOT NULL,
        created_at TEXT NOT NULL,
        FOREIGN KEY(run_id) REFERENCES runs(id) ON DELETE CASCADE,
        FOREIGN KEY(job_id) REFERENCES jobs(id) ON DELETE SET NULL
    );
    CREATE TABLE remote_tasks (
        id TEXT PRIMARY KEY NOT NULL,
        run_id TEXT NOT NULL,
        job_id TEXT,
        provider_profile_id TEXT NOT NULL,
        remote_id TEXT NOT NULL,
        task_type TEXT NOT NULL,
        status TEXT NOT NULL,
        next_poll_at TEXT,
        task_json TEXT NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        FOREIGN KEY(run_id) REFERENCES runs(id) ON DELETE CASCADE,
        FOREIGN KEY(job_id) REFERENCES jobs(id) ON DELETE SET NULL
    );
    CREATE UNIQUE INDEX remote_tasks_provider_id_idx ON remote_tasks(provider_profile_id, remote_id);
    CREATE TABLE remote_files (
        id TEXT PRIMARY KEY NOT NULL,
        provider_profile_id TEXT NOT NULL,
        provider_file_id TEXT NOT NULL,
        purpose TEXT,
        expires_at TEXT,
        file_json TEXT NOT NULL,
        created_at TEXT NOT NULL
    );
    CREATE UNIQUE INDEX remote_files_provider_id_idx ON remote_files(provider_profile_id, provider_file_id);
    "#];

pub(crate) async fn migrate(pool: &SqlitePool, migrations: &[&str]) -> StorageResult<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY NOT NULL, applied_at TEXT NOT NULL)",
    )
    .execute(pool)
    .await?;
    for (index, migration) in migrations.iter().enumerate() {
        let version = (index + 1) as i64;
        let applied: Option<i64> =
            sqlx::query_scalar("SELECT version FROM schema_migrations WHERE version = ? LIMIT 1")
                .bind(version)
                .fetch_optional(pool)
                .await?;
        if applied.is_some() {
            continue;
        }
        let mut transaction = pool.begin().await?;
        execute_statements(&mut transaction, migration).await?;
        sqlx::query("INSERT INTO schema_migrations(version, applied_at) VALUES(?, ?)")
            .bind(version)
            .bind(chrono::Utc::now().to_rfc3339())
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
    }
    Ok(())
}

async fn execute_statements(
    transaction: &mut Transaction<'_, Sqlite>,
    migration: &str,
) -> StorageResult<()> {
    for statement in migration
        .split(';')
        .map(str::trim)
        .filter(|sql| !sql.is_empty())
    {
        sqlx::query(statement).execute(&mut **transaction).await?;
    }
    Ok(())
}
