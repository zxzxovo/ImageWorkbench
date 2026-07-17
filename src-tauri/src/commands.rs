use std::collections::{BTreeMap, HashSet};

use base64::Engine;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

use crate::app_state::AppState;
use crate::bridge::{
    CommandError, CommandResult, DescriptionPlacement, GenerateImagesRequest, GeneratedAsset,
    GenerationCommandResult, GenerationEventEnvelope, GenerationMode, HistoryDetailsDto,
    HistoryOutputDto, ImportedInputDto, ProjectDetailsDto, ProviderProfileDto, WorkspaceSnapshot,
    parse_timestamp,
};
use crate::domain::{
    ContextPlacement, ErrorRecord, GenerationPreset, ImageSize, JobStatus, Operation, OutputSpec,
    ProjectSummary, PromptContext, RunRecord, RunStatus,
};
use crate::providers::{ProviderAdapter, create_adapter};
use crate::runtime::{execute_generation, poll_remote_tasks};
use crate::storage::{ProviderRemapResult, RemoteFileRecord, strip_extended_length_prefix};

const WORKSPACE_SETTING_KEY: &str = "workspace.v1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleFailureDto {
    pub run_id: Option<String>,
    pub provider_profile_id: Option<String>,
    pub remote_file_id: Option<String>,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HistoryMutationResultDto {
    pub requested_runs: u32,
    pub deleted_runs: u32,
    pub local_assets_deleted: u32,
    pub remote_files_deleted: u32,
    pub remote_files_retained: u32,
    #[serde(default)]
    pub failures: Vec<LifecycleFailureDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticProjectDto {
    pub id: String,
    pub name: String,
    pub storage_path: String,
    pub database_exists: bool,
    pub is_open: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticReportDto {
    pub app_version: String,
    pub os: String,
    pub architecture: String,
    pub app_data_directory: String,
    pub log_directory: String,
    pub log_files: Vec<String>,
    pub log_tail: String,
    pub credential_store_status: String,
    pub credential_store_message: String,
    pub projects: Vec<DiagnosticProjectDto>,
}

#[tauri::command]
#[specta::specta]
pub async fn workspace_load(
    state: tauri::State<'_, AppState>,
) -> CommandResult<Option<WorkspaceSnapshot>> {
    let snapshot = state
        .global_store()
        .setting::<WorkspaceSnapshot>(WORKSPACE_SETTING_KEY)
        .await?;
    if let Some(snapshot) = &snapshot {
        for project in &snapshot.projects {
            let database = std::path::Path::new(&project.storage_path)
                .join(".imageworkbench")
                .join("project.sqlite3");
            if database.is_file() {
                if let Err(error) = state.open_project(&project.storage_path).await {
                    tracing::warn!(project_id = %project.id, path = %project.storage_path, %error, "failed to reopen project");
                }
            } else {
                tracing::warn!(project_id = %project.id, path = %project.storage_path, "project database is missing during workspace restore");
            }
        }
    }
    Ok(snapshot.map(|snapshot| snapshot.for_global_storage()))
}

#[tauri::command]
#[specta::specta]
pub async fn workspace_save(
    state: tauri::State<'_, AppState>,
    snapshot: WorkspaceSnapshot,
) -> CommandResult<()> {
    save_workspace_snapshot(&state, snapshot).await
}

#[tauri::command]
#[specta::specta]
pub async fn diagnostics_report(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> CommandResult<DiagnosticReportDto> {
    let app_data_directory = app
        .path()
        .app_data_dir()
        .map_err(|error| CommandError::new("app_data", error.to_string()))?;
    let log_directory = app_data_directory.join("logs");
    let (credential_store_status, credential_store_message) = match state
        .credential_store_health()
        .await
    {
        Ok(()) => (
            "ok".to_owned(),
            "Credential round-trip succeeded".to_owned(),
        ),
        Err(error) => {
            tracing::error!(code = %error.code, message = %error.message, "credential store diagnostic failed");
            ("error".to_owned(), error.to_string())
        }
    };
    let (log_files, log_tail) = read_diagnostic_logs(&log_directory);

    let open_projects = state
        .open_project_summaries()
        .await?
        .into_iter()
        .map(|summary| summary.id)
        .collect::<HashSet<_>>();
    let workspace = state
        .global_store()
        .setting::<WorkspaceSnapshot>(WORKSPACE_SETTING_KEY)
        .await?;
    let projects = workspace
        .map(|snapshot| snapshot.projects)
        .unwrap_or_default()
        .into_iter()
        .map(|project| {
            let database_exists = std::path::Path::new(&project.storage_path)
                .join(".imageworkbench")
                .join("project.sqlite3")
                .is_file();
            DiagnosticProjectDto {
                is_open: open_projects.contains(&project.id),
                database_exists,
                id: project.id,
                name: project.name,
                storage_path: project.storage_path,
            }
        })
        .collect();

    Ok(DiagnosticReportDto {
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        os: std::env::consts::OS.to_owned(),
        architecture: std::env::consts::ARCH.to_owned(),
        app_data_directory: app_data_directory.to_string_lossy().into_owned(),
        log_directory: log_directory.to_string_lossy().into_owned(),
        log_files,
        log_tail,
        credential_store_status,
        credential_store_message,
        projects,
    })
}

#[tauri::command]
#[specta::specta]
pub fn diagnostics_open_logs(app: AppHandle) -> CommandResult<()> {
    let log_directory = app
        .path()
        .app_data_dir()
        .map_err(|error| CommandError::new("app_data", error.to_string()))?
        .join("logs");
    std::fs::create_dir_all(&log_directory)?;
    app.opener()
        .open_path(log_directory.to_string_lossy(), None::<String>)
        .map_err(|error| CommandError::new("opener", error.to_string()))
}

fn read_diagnostic_logs(log_directory: &std::path::Path) -> (Vec<String>, String) {
    let mut files = std::fs::read_dir(log_directory)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("imageworkbench.log")
        })
        .collect::<Vec<_>>();
    files.sort_by_key(|entry| {
        entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
    });
    let names = files
        .iter()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let Some(latest) = files.last() else {
        return (names, String::new());
    };
    let bytes = std::fs::read(latest.path()).unwrap_or_default();
    let start = bytes.len().saturating_sub(128 * 1024);
    let tail = String::from_utf8_lossy(&bytes[start..]).into_owned();
    (names, tail)
}

async fn save_workspace_snapshot(
    state: &AppState,
    snapshot: WorkspaceSnapshot,
) -> CommandResult<()> {
    for provider in &snapshot.providers {
        if !provider.api_key.trim().is_empty() {
            state
                .set_secret(
                    provider.credential_key(),
                    provider.api_key.trim().to_owned(),
                )
                .await?;
        }
        for header in &provider.custom_headers {
            if header.secret && !header.value.is_empty() {
                state
                    .set_secret(header.credential_key(&provider.id), header.value.clone())
                    .await?;
            }
        }
        let mut profile = provider.to_domain_profile()?;
        if let Some(existing) = state.global_store().provider(&profile.id).await? {
            profile.created_at = existing.created_at;
        }
        state.global_store().upsert_provider(&profile).await?;
    }
    let sanitized = snapshot.without_api_keys();
    sync_workspace_projects(state, &sanitized).await?;
    let global_snapshot = sanitized.for_global_storage();
    state
        .global_store()
        .set_setting(WORKSPACE_SETTING_KEY, &global_snapshot)
        .await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn queue_pause(state: tauri::State<'_, AppState>) -> bool {
    state.pause_queue();
    state.queue_is_paused()
}

#[tauri::command]
#[specta::specta]
pub fn queue_resume(state: tauri::State<'_, AppState>) -> bool {
    state.resume_queue();
    state.queue_is_paused()
}

#[tauri::command]
#[specta::specta]
pub fn queue_status(state: tauri::State<'_, AppState>) -> bool {
    state.queue_is_paused()
}

#[tauri::command]
#[specta::specta]
pub async fn provider_secret_get(
    state: tauri::State<'_, AppState>,
    provider_id: String,
) -> CommandResult<bool> {
    match state
        .secret(crate::security::CredentialKey::provider(&provider_id))
        .await
    {
        Ok(_) => Ok(true),
        Err(error) if error.code == "not_found" => Ok(false),
        Err(error) => Err(error),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn provider_secret_exists(
    state: tauri::State<'_, AppState>,
    provider_id: String,
) -> CommandResult<bool> {
    provider_secret_get(state, provider_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn provider_secret_set(
    state: tauri::State<'_, AppState>,
    provider_id: String,
    api_key: String,
) -> CommandResult<()> {
    if provider_id.trim().is_empty() {
        return Err(CommandError::validation("provider ID is required"));
    }
    state
        .set_secret(
            crate::security::CredentialKey::provider(&provider_id),
            api_key,
        )
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn provider_secret_delete(
    state: tauri::State<'_, AppState>,
    provider_id: String,
) -> CommandResult<()> {
    state
        .delete_secret(crate::security::CredentialKey::provider(&provider_id))
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn provider_header_secret_set(
    state: tauri::State<'_, AppState>,
    provider_id: String,
    header_id: String,
    value: String,
) -> CommandResult<()> {
    let key = crate::security::CredentialKey {
        service: "dev.imageworkbench.desktop".to_owned(),
        account: format!("provider-header:{provider_id}:{header_id}"),
    };
    state.set_secret(key, value).await
}

#[tauri::command]
#[specta::specta]
pub async fn provider_header_secret_exists(
    state: tauri::State<'_, AppState>,
    provider_id: String,
    header_id: String,
) -> CommandResult<bool> {
    let key = crate::security::CredentialKey {
        service: "dev.imageworkbench.desktop".to_owned(),
        account: format!("provider-header:{provider_id}:{header_id}"),
    };
    match state.secret(key).await {
        Ok(_) => Ok(true),
        Err(error) if error.code == "not_found" => Ok(false),
        Err(error) => Err(error),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn provider_header_secret_delete(
    state: tauri::State<'_, AppState>,
    provider_id: String,
    header_id: String,
) -> CommandResult<()> {
    state
        .delete_secret(crate::security::CredentialKey {
            service: "dev.imageworkbench.desktop".to_owned(),
            account: format!("provider-header:{provider_id}:{header_id}"),
        })
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn provider_delete(
    state: tauri::State<'_, AppState>,
    provider_id: String,
) -> CommandResult<bool> {
    delete_provider_data(&state, &provider_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn provider_test(
    state: tauri::State<'_, AppState>,
    provider: ProviderProfileDto,
) -> CommandResult<bool> {
    let adapter = adapter_for_profile(&state, &provider).await?;
    let limit = state.provider_limit(&provider.id).await;
    let _permit = limit
        .acquire_owned()
        .await
        .map_err(|_| CommandError::new("queue_closed", "provider queue is closed"))?;
    adapter.test_connection().await?;
    Ok(true)
}

#[tauri::command]
#[specta::specta]
pub async fn provider_sync_models(
    state: tauri::State<'_, AppState>,
    provider: ProviderProfileDto,
) -> CommandResult<Vec<String>> {
    let adapter = adapter_for_profile(&state, &provider).await?;
    let limit = state.provider_limit(&provider.id).await;
    let _permit = limit
        .acquire_owned()
        .await
        .map_err(|_| CommandError::new("queue_closed", "provider queue is closed"))?;
    let kind = provider.adapter_kind();
    let discovered = adapter.list_models().await?;
    let mut models = Vec::with_capacity(discovered.len());
    for model in discovered {
        if is_image_model_slug(kind, &model.id)? {
            models.push(model.id);
        }
    }
    models.sort();
    models.dedup();
    Ok(models)
}

#[tauri::command]
#[specta::specta]
pub async fn generate_images(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    mut request: GenerateImagesRequest,
) -> CommandResult<GenerationCommandResult> {
    ensure_client_task_id(&mut request);
    let handler = generation_event_handler(app, &request);
    execute_generation(&state, request, Some(handler)).await
}

#[tauri::command]
#[specta::specta]
pub async fn generate_images_legacy(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    mut request: GenerateImagesRequest,
) -> CommandResult<Vec<GeneratedAsset>> {
    ensure_client_task_id(&mut request);
    let handler = generation_event_handler(app, &request);
    Ok(execute_generation(&state, request, Some(handler))
        .await?
        .assets)
}

#[tauri::command]
#[specta::specta]
pub async fn reveal_path(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    path: String,
) -> CommandResult<()> {
    let path = state.validate_open_project_path(path).await?;
    if path.is_file() {
        app.opener()
            .reveal_item_in_dir(&path)
            .map_err(|error| CommandError::new("opener", error.to_string()))?;
    } else {
        app.opener()
            .open_path(path.to_string_lossy(), None::<String>)
            .map_err(|error| CommandError::new("opener", error.to_string()))?;
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn export_asset(
    state: tauri::State<'_, AppState>,
    source_path: String,
    destination_path: String,
) -> CommandResult<()> {
    let source = state
        .validate_open_project_path(source_path.clone())
        .await
        .map_err(|error| {
            tracing::warn!(%error, source_path, "export_asset: path validation failed");
            error
        })?;
    if !source.is_file() {
        tracing::warn!(source = %source.display(), "export_asset: resolved path is not a file");
        return Err(CommandError::validation("export source is not a file"));
    }

    let destination = std::path::PathBuf::from(destination_path);
    if destination.as_os_str().is_empty() || destination.file_name().is_none() {
        return Err(CommandError::validation("export destination is invalid"));
    }
    if destination.exists() && destination.canonicalize().is_ok_and(|path| path == source) {
        return Ok(());
    }
    let parent = destination
        .parent()
        .ok_or_else(|| CommandError::validation("export destination has no parent directory"))?;
    if !parent.is_dir() {
        return Err(CommandError::validation(
            "export destination directory does not exist",
        ));
    }

    tokio::fs::copy(&source, &destination)
        .await
        .map_err(|error| CommandError::new("asset_export", error.to_string()))?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn project_create(
    state: tauri::State<'_, AppState>,
    project_id: String,
    name: String,
    path: String,
) -> CommandResult<ProjectSummary> {
    let store = state.create_project(&project_id, &name, &path).await.map_err(|error| {
        tracing::error!(code = %error.code, message = %error.message, %project_id, %path, "project creation failed");
        error
    })?;
    Ok(store.summary().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn project_open(
    state: tauri::State<'_, AppState>,
    path: String,
) -> CommandResult<ProjectSummary> {
    let store = state.open_project(&path).await.map_err(|error| {
        tracing::error!(code = %error.code, message = %error.message, %path, "project open failed");
        error
    })?;
    Ok(store.summary().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn project_load_details(
    state: tauri::State<'_, AppState>,
    project_id: String,
    recent_run_limit: Option<u32>,
) -> CommandResult<ProjectDetailsDto> {
    let store = state.project(&project_id).await?;
    let runs = store
        .list_runs(recent_run_limit.unwrap_or(50).clamp(1, 500), 0)
        .await?;
    let mut recent_records = Vec::with_capacity(runs.len());
    for run in runs {
        recent_records.push(load_history_details(&store, run).await?);
    }
    Ok(ProjectDetailsDto {
        summary: store.summary().await?,
        contexts: store.list_prompt_contexts().await?,
        presets: store.list_presets().await?,
        recent_records,
    })
}

#[tauri::command]
#[specta::specta]
pub async fn project_close(
    state: tauri::State<'_, AppState>,
    project_id: String,
) -> CommandResult<bool> {
    Ok(state.close_project(&project_id).await)
}

#[tauri::command]
#[specta::specta]
pub async fn projects_open_list(
    state: tauri::State<'_, AppState>,
) -> CommandResult<Vec<ProjectSummary>> {
    state.open_project_summaries().await
}

#[tauri::command]
#[specta::specta]
pub async fn project_import_inputs(
    state: tauri::State<'_, AppState>,
    project_id: String,
    paths: Vec<String>,
) -> CommandResult<Vec<ImportedInputDto>> {
    let project = state.project(&project_id).await?;
    let mut imported = Vec::with_capacity(paths.len());
    for path in paths {
        let path = std::path::PathBuf::from(path).canonicalize()?;
        if !path.is_file() {
            return Err(CommandError::validation(format!(
                "{} is not a file",
                path.display()
            )));
        }
        let stored = crate::storage::import_input_file(project.layout(), &path)?;
        let relative = stored
            .path
            .strip_prefix(project.layout().root())
            .map_err(|_| CommandError::validation("imported file escaped the project root"))?;
        let mime_type = mime_for_path(&stored.path);
        let dimensions = mime_type
            .starts_with("image/")
            .then(|| image::image_dimensions(&stored.path).ok())
            .flatten();
        imported.push(ImportedInputDto {
            relative_path: relative.to_string_lossy().into_owned(),
            mime_type: mime_type.to_owned(),
            sha256: stored.sha256,
            size_bytes: stored.size_bytes,
            width: dimensions.map(|dimensions| dimensions.0),
            height: dimensions.map(|dimensions| dimensions.1),
        });
    }
    Ok(imported)
}

#[tauri::command]
#[specta::specta]
pub async fn reference_preview(
    state: tauri::State<'_, AppState>,
    project_id: String,
    path: String,
) -> CommandResult<Option<String>> {
    let project = state.project(&project_id).await?;
    let path = validated_project_asset_path(&project, &path)?;
    let metadata = std::fs::metadata(&path)?;
    if metadata.len() > 50 * 1024 * 1024 {
        return Err(CommandError::validation("preview input exceeds 50 MB"));
    }
    if mime_for_path(&path).starts_with("video/") {
        return Ok(None);
    }
    let image = image::open(&path)
        .map_err(|error| CommandError::validation(format!("preview image is invalid: {error}")))?;
    let thumbnail = image.thumbnail(512, 512);
    let mut bytes = std::io::Cursor::new(Vec::new());
    thumbnail
        .write_to(&mut bytes, image::ImageFormat::Png)
        .map_err(|error| CommandError::new("preview_encode", error.to_string()))?;
    Ok(Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())
    )))
}

#[tauri::command]
#[specta::specta]
pub async fn project_asset_data_url(
    state: tauri::State<'_, AppState>,
    project_id: String,
    path: String,
) -> CommandResult<Option<String>> {
    let project = state.project(&project_id).await?;
    let path = validated_project_asset_path(&project, &path)?;
    let metadata = std::fs::metadata(&path)?;
    if metadata.len() > 50 * 1024 * 1024 {
        return Err(CommandError::validation("project asset exceeds 50 MB"));
    }
    let mime = mime_for_path(&path);
    if mime.starts_with("video/") {
        return Ok(None);
    }
    let bytes = std::fs::read(&path)?;
    image::load_from_memory(&bytes)
        .map_err(|error| CommandError::validation(format!("project image is invalid: {error}")))?;
    Ok(Some(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )))
}

fn validated_project_asset_path(
    project: &crate::storage::ProjectStore,
    path: &str,
) -> CommandResult<std::path::PathBuf> {
    let path = std::path::PathBuf::from(path);
    let path = if path.is_absolute() {
        path
    } else {
        project.layout().root().join(path)
    }
    .canonicalize()?;
    if !path.starts_with(project.layout().root()) || !path.is_file() {
        return Err(CommandError::validation(
            "asset path must be a file inside the project",
        ));
    }
    Ok(path)
}

#[tauri::command]
#[specta::specta]
pub async fn prompt_context_list(
    state: tauri::State<'_, AppState>,
    project_id: String,
) -> CommandResult<Vec<PromptContext>> {
    Ok(state
        .project(&project_id)
        .await?
        .list_prompt_contexts()
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn prompt_context_upsert(
    state: tauri::State<'_, AppState>,
    project_id: String,
    context: PromptContext,
) -> CommandResult<()> {
    state
        .project(&project_id)
        .await?
        .upsert_prompt_context(&context)
        .await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn prompt_context_delete(
    state: tauri::State<'_, AppState>,
    project_id: String,
    context_id: String,
) -> CommandResult<bool> {
    Ok(state
        .project(&project_id)
        .await?
        .delete_prompt_context(&context_id)
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn generation_preset_list(
    state: tauri::State<'_, AppState>,
    project_id: String,
) -> CommandResult<Vec<GenerationPreset>> {
    Ok(state.project(&project_id).await?.list_presets().await?)
}

#[tauri::command]
#[specta::specta]
pub async fn generation_preset_upsert(
    state: tauri::State<'_, AppState>,
    project_id: String,
    preset: GenerationPreset,
) -> CommandResult<()> {
    state
        .project(&project_id)
        .await?
        .upsert_preset(&preset)
        .await?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn generation_preset_delete(
    state: tauri::State<'_, AppState>,
    project_id: String,
    preset_id: String,
) -> CommandResult<bool> {
    Ok(state
        .project(&project_id)
        .await?
        .delete_preset(&preset_id)
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn history_list(
    state: tauri::State<'_, AppState>,
    project_id: String,
    limit: u32,
    offset: u32,
) -> CommandResult<Vec<RunRecord>> {
    Ok(state
        .project(&project_id)
        .await?
        .list_runs(limit, offset)
        .await?)
}

#[tauri::command]
#[specta::specta]
pub async fn history_get(
    state: tauri::State<'_, AppState>,
    project_id: String,
    run_id: String,
) -> CommandResult<Option<RunRecord>> {
    Ok(state.project(&project_id).await?.run(&run_id).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn history_details(
    state: tauri::State<'_, AppState>,
    project_id: String,
    run_id: String,
) -> CommandResult<Option<HistoryDetailsDto>> {
    let store = state.project(&project_id).await?;
    let Some(run) = store.run(&run_id).await? else {
        return Ok(None);
    };
    Ok(Some(load_history_details(&store, run).await?))
}

#[tauri::command]
#[specta::specta]
pub async fn run_cancel(
    state: tauri::State<'_, AppState>,
    run_id: String,
    project_id: Option<String>,
) -> CommandResult<bool> {
    if state.cancel(&run_id).await {
        return Ok(true);
    }
    let Some(project_id) = project_id.filter(|value| !value.trim().is_empty()) else {
        return Ok(false);
    };
    cancel_remote_run(&state, &project_id, &run_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn history_delete(
    state: tauri::State<'_, AppState>,
    project_id: String,
    run_id: String,
    delete_local_assets: bool,
    delete_remote_files: bool,
) -> CommandResult<HistoryMutationResultDto> {
    delete_history_runs(
        &state,
        &project_id,
        vec![run_id],
        delete_local_assets,
        delete_remote_files,
    )
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn history_clear(
    state: tauri::State<'_, AppState>,
    project_id: String,
    delete_local_assets: bool,
    delete_remote_files: bool,
) -> CommandResult<HistoryMutationResultDto> {
    let run_ids = state.project(&project_id).await?.run_ids().await?;
    delete_history_runs(
        &state,
        &project_id,
        run_ids,
        delete_local_assets,
        delete_remote_files,
    )
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn project_remap_provider(
    state: tauri::State<'_, AppState>,
    project_id: String,
    from_provider_id: String,
    to_provider_id: String,
) -> CommandResult<ProviderRemapResult> {
    if state
        .global_store()
        .provider(&to_provider_id)
        .await?
        .is_none()
    {
        return Err(CommandError::not_found(format!(
            "target provider profile {to_provider_id} is missing"
        )));
    }
    let project = state.project(&project_id).await?;
    let result = project
        .remap_provider(&from_provider_id, &to_provider_id)
        .await?;
    state
        .global_store()
        .add_recent_project(&project.summary().await?)
        .await?;
    Ok(result)
}

#[tauri::command]
#[specta::specta]
pub async fn remote_tasks_poll(
    state: tauri::State<'_, AppState>,
    project_id: String,
) -> CommandResult<Vec<GenerationCommandResult>> {
    poll_remote_tasks(&state, &project_id).await
}

async fn adapter_for_profile(
    state: &AppState,
    profile: &ProviderProfileDto,
) -> CommandResult<Box<dyn ProviderAdapter>> {
    let api_key = if profile.api_key.trim().is_empty() {
        state.secret(profile.credential_key()).await?
    } else {
        profile.api_key.trim().to_owned()
    };
    Ok(create_adapter(
        state.resolved_provider_config(profile).await?,
        profile.credentials(api_key),
    )?)
}

async fn adapter_for_stored_profile(
    state: &AppState,
    provider_profile_id: &str,
) -> CommandResult<Box<dyn ProviderAdapter>> {
    let profile = state
        .global_store()
        .provider(provider_profile_id)
        .await?
        .ok_or_else(|| {
            CommandError::not_found(format!("provider profile {provider_profile_id} is missing"))
        })?;
    let kind = match profile.kind {
        crate::domain::ProviderKind::OpenAi => crate::providers::types::ProviderKind::OpenAi,
        crate::domain::ProviderKind::XAi => crate::providers::types::ProviderKind::Xai,
        crate::domain::ProviderKind::Gemini => crate::providers::types::ProviderKind::Gemini,
        crate::domain::ProviderKind::OpenAiCompatible => {
            crate::providers::types::ProviderKind::OpenAiCompatible
        }
    };
    let auth = match &profile.auth_scheme {
        crate::domain::AuthScheme::Bearer => crate::providers::types::AuthScheme::Bearer,
        crate::domain::AuthScheme::Header { name, prefix } => {
            crate::providers::types::AuthScheme::Header {
                name: name.clone(),
                prefix: prefix.clone(),
            }
        }
        crate::domain::AuthScheme::QueryParameter { name } => {
            crate::providers::types::AuthScheme::Query { name: name.clone() }
        }
        crate::domain::AuthScheme::None => {
            return Err(CommandError::validation(
                "provider profile has no API authentication scheme",
            ));
        }
    };
    let mut headers = profile.custom_headers.clone();
    for (name, reference) in &profile.secret_header_refs {
        let key = credential_key_from_reference(reference)
            .ok_or_else(|| CommandError::validation("invalid keyring reference"))?;
        headers.insert(name.clone(), state.secret(key).await?);
    }
    let credential_key = profile
        .credential_ref
        .as_deref()
        .and_then(credential_key_from_reference)
        .unwrap_or_else(|| crate::security::CredentialKey::provider(&profile.id));
    let api_key = state.secret(credential_key).await?;
    Ok(create_adapter(
        crate::providers::types::ProviderConfig {
            id: profile.id,
            name: profile.name,
            kind,
            base_url: profile.base_url,
            auth,
            headers,
            timeout_secs: profile.timeout_ms.saturating_add(999) / 1_000,
            proxy_url: profile.proxy_url,
            organization: profile.organization,
            project: profile.project,
            api_version: profile.api_version,
            models_path: profile.models_path,
        },
        crate::providers::types::ProviderCredentials { api_key },
    )?)
}

async fn cancel_remote_run(
    state: &AppState,
    project_id: &str,
    run_id: &str,
) -> CommandResult<bool> {
    let project = state.project(project_id).await?;
    let Some(mut run) = project.run(run_id).await? else {
        return Ok(false);
    };
    let tasks = project
        .remote_tasks_for_run(run_id)
        .await?
        .into_iter()
        .filter(|task| {
            !matches!(
                task.status.as_str(),
                "succeeded" | "failed" | "cancelled" | "expired"
            )
        })
        .collect::<Vec<_>>();
    if tasks.is_empty() {
        return Ok(false);
    }

    let mut cancelled = 0_u32;
    let mut failures = Vec::new();
    for task in &tasks {
        let outcome: CommandResult<()> = async {
            let adapter = adapter_for_stored_profile(state, &task.provider_profile_id).await?;
            let remote = crate::providers::types::RemoteJob {
                id: task.remote_id.clone(),
                kind: if task.task_type == "batch" {
                    crate::providers::types::RemoteJobKind::Batch
                } else {
                    crate::providers::types::RemoteJobKind::Background
                },
                status: provider_remote_status(&task.status),
                provider: adapter.kind(),
                model: Some(run.request.model_id.clone()),
                raw: Value::Object(task.metadata.clone().into_iter().collect()),
            };
            let response = adapter.cancel_job(&remote).await?;
            let mut metadata = crate::security::redact_json(&response.raw)
                .as_object()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .collect::<BTreeMap<_, _>>();
            metadata.insert(
                "cancelledAt".to_owned(),
                Value::String(Utc::now().to_rfc3339()),
            );
            project
                .update_remote_task_status(&task.id, "cancelled", None, Some(metadata))
                .await?;
            let mut jobs = project.list_jobs(run_id).await?;
            for job in &mut jobs {
                let matches_task = task.job_id.as_deref() == Some(job.id.as_str())
                    || job.remote_job_id.as_deref() == Some(task.remote_id.as_str())
                    || job.remote_batch_id.as_deref() == Some(task.remote_id.as_str());
                if matches_task {
                    job.status = JobStatus::Cancelled;
                    job.updated_at = Utc::now();
                    project.upsert_job(job).await?;
                }
            }
            Ok(())
        }
        .await;
        match outcome {
            Ok(()) => cancelled = cancelled.saturating_add(1),
            Err(error) => {
                project
                    .save_error(&ErrorRecord {
                        id: uuid::Uuid::new_v4().to_string(),
                        run_id: run.id.clone(),
                        job_id: task.job_id.clone(),
                        error: crate::domain::ProviderError {
                            code: error.code.clone(),
                            message: error.message.clone(),
                            http_status: None,
                            retryable: true,
                            request_id: Some(task.remote_id.clone()),
                            provider: None,
                            details: error.details.clone(),
                        },
                        created_at: Utc::now(),
                    })
                    .await?;
                failures.push(LifecycleFailureDto {
                    run_id: Some(run.id.clone()),
                    provider_profile_id: Some(task.provider_profile_id.clone()),
                    remote_file_id: None,
                    code: error.code,
                    message: error.message,
                });
            }
        }
    }
    if cancelled == tasks.len() as u32 {
        run.status = RunStatus::Cancelled;
        run.finished_at = Some(Utc::now());
        run.updated_at = Utc::now();
        project.upsert_run(&run).await?;
    }
    if !failures.is_empty() {
        let mut error = CommandError::new(
            "remote_cancel_failed",
            format!("cancelled {cancelled} of {} remote tasks", tasks.len()),
        );
        error.details = Some(json!({ "failures": failures }));
        return Err(error);
    }
    Ok(cancelled > 0)
}

async fn delete_history_runs(
    state: &AppState,
    project_id: &str,
    run_ids: Vec<String>,
    delete_local_assets: bool,
    delete_remote_files: bool,
) -> CommandResult<HistoryMutationResultDto> {
    let project = state.project(project_id).await?;
    let mut result = HistoryMutationResultDto {
        requested_runs: run_ids.len() as u32,
        ..HistoryMutationResultDto::default()
    };
    for run_id in run_ids {
        let Some(run) = project.run(&run_id).await? else {
            continue;
        };
        if delete_local_assets && let Err(error) = project.validate_run_local_assets(&run_id).await
        {
            let error: CommandError = error.into();
            result.failures.push(LifecycleFailureDto {
                run_id: Some(run_id),
                provider_profile_id: Some(run.request.provider_profile_id),
                remote_file_id: None,
                code: error.code,
                message: error.message,
            });
            continue;
        }

        if delete_remote_files {
            delete_run_remote_files(state, &project, &run, &mut result).await?;
        }
        match project.delete_run(&run.id, delete_local_assets).await {
            Ok(deleted) => {
                if deleted.deleted {
                    result.deleted_runs = result.deleted_runs.saturating_add(1);
                    result.local_assets_deleted = result
                        .local_assets_deleted
                        .saturating_add(deleted.local_assets_deleted);
                }
            }
            Err(error) => {
                let error: CommandError = error.into();
                result.failures.push(LifecycleFailureDto {
                    run_id: Some(run.id),
                    provider_profile_id: Some(run.request.provider_profile_id),
                    remote_file_id: None,
                    code: error.code,
                    message: error.message,
                });
            }
        }
    }
    Ok(result)
}

async fn delete_run_remote_files(
    state: &AppState,
    project: &crate::storage::ProjectStore,
    run: &RunRecord,
    result: &mut HistoryMutationResultDto,
) -> CommandResult<()> {
    let provider_file_ids = project
        .list_outputs(&run.id)
        .await?
        .into_iter()
        .filter_map(|output| output.provider_file_id)
        .collect::<HashSet<_>>();
    let records = project
        .remote_files_for_run(&run.id)
        .await?
        .into_iter()
        .map(|file| (file.provider_file_id.clone(), file))
        .collect::<BTreeMap<_, _>>();
    for provider_file_id in provider_file_ids {
        let record = records.get(&provider_file_id);
        let provider_profile_id = record
            .map(|file| file.provider_profile_id.as_str())
            .unwrap_or(&run.request.provider_profile_id);
        if project
            .remote_file_is_referenced_elsewhere(&run.id, provider_profile_id, &provider_file_id)
            .await?
        {
            result.remote_files_retained = result.remote_files_retained.saturating_add(1);
            continue;
        }
        let outcome: CommandResult<()> = async {
            let adapter = adapter_for_stored_profile(state, provider_profile_id).await?;
            adapter.delete_file(&provider_file_id).await?;
            Ok(())
        }
        .await;
        match outcome {
            Ok(()) => {
                if let Some(record) = record {
                    project.delete_remote_file(&record.id).await?;
                }
                result.remote_files_deleted = result.remote_files_deleted.saturating_add(1);
            }
            Err(error) => {
                record_remote_file_delete_failure(project, record, &error).await?;
                tracing::warn!(
                    run_id = %run.id,
                    provider_profile_id,
                    remote_file_id = %provider_file_id,
                    code = %error.code,
                    "remote file deletion failed"
                );
                result.failures.push(LifecycleFailureDto {
                    run_id: Some(run.id.clone()),
                    provider_profile_id: Some(provider_profile_id.to_owned()),
                    remote_file_id: Some(provider_file_id),
                    code: error.code,
                    message: error.message,
                });
            }
        }
    }
    Ok(())
}

async fn record_remote_file_delete_failure(
    project: &crate::storage::ProjectStore,
    record: Option<&RemoteFileRecord>,
    error: &CommandError,
) -> CommandResult<()> {
    if let Some(record) = record {
        project
            .record_remote_file_delete_error(
                &record.id,
                json!({ "code": error.code, "message": error.message }),
            )
            .await?;
    }
    Ok(())
}

fn provider_remote_status(status: &str) -> crate::providers::types::RemoteJobStatus {
    match status {
        "queued" => crate::providers::types::RemoteJobStatus::Queued,
        "running" => crate::providers::types::RemoteJobStatus::Running,
        "succeeded" => crate::providers::types::RemoteJobStatus::Succeeded,
        "failed" => crate::providers::types::RemoteJobStatus::Failed,
        "cancelled" => crate::providers::types::RemoteJobStatus::Cancelled,
        "expired" => crate::providers::types::RemoteJobStatus::Expired,
        _ => crate::providers::types::RemoteJobStatus::Unknown,
    }
}

async fn delete_provider_data(state: &AppState, provider_id: &str) -> CommandResult<bool> {
    if provider_id.trim().is_empty() {
        return Err(CommandError::validation("provider ID is required"));
    }
    let stored_profile = state.global_store().provider(provider_id).await?;
    let mut workspace = state
        .global_store()
        .setting::<WorkspaceSnapshot>(WORKSPACE_SETTING_KEY)
        .await?;
    let mut credential_keys =
        HashSet::from([crate::security::CredentialKey::provider(provider_id)]);
    if let Some(profile) = &stored_profile {
        if let Some(key) = profile
            .credential_ref
            .as_deref()
            .and_then(credential_key_from_reference)
        {
            credential_keys.insert(key);
        }
        for reference in profile.secret_header_refs.values() {
            if let Some(key) = credential_key_from_reference(reference) {
                credential_keys.insert(key);
            }
        }
    }
    if let Some(snapshot) = &workspace
        && let Some(profile) = snapshot
            .providers
            .iter()
            .find(|profile| profile.id == provider_id)
    {
        for header in &profile.custom_headers {
            if header.secret {
                credential_keys.insert(header.credential_key(provider_id));
            }
        }
    }
    let mut removed_secret = false;
    for key in credential_keys {
        match state.delete_secret(key).await {
            Ok(()) => removed_secret = true,
            Err(error) if error.code == "not_found" => {}
            Err(error) => return Err(error),
        }
    }
    let removed_profile = state.global_store().delete_provider(provider_id).await?;
    let mut removed_workspace_profile = false;
    if let Some(snapshot) = &mut workspace {
        let previous_len = snapshot.providers.len();
        snapshot
            .providers
            .retain(|profile| profile.id != provider_id);
        removed_workspace_profile = snapshot.providers.len() != previous_len;
        if removed_workspace_profile {
            state
                .global_store()
                .set_setting(WORKSPACE_SETTING_KEY, &snapshot.for_global_storage())
                .await?;
        }
    }
    Ok(removed_profile || removed_workspace_profile || removed_secret)
}

fn credential_key_from_reference(reference: &str) -> Option<crate::security::CredentialKey> {
    let (service, account) = reference.strip_prefix("keyring://")?.split_once('/')?;
    (!service.is_empty() && !account.is_empty()).then(|| crate::security::CredentialKey {
        service: service.to_owned(),
        account: account.to_owned(),
    })
}

fn is_image_model_slug(
    kind: crate::providers::types::ProviderKind,
    model_id: &str,
) -> CommandResult<bool> {
    if crate::providers::capabilities::find_model(kind, model_id)?
        .is_some_and(|capability| capability.id != "*")
    {
        return Ok(true);
    }
    let slug = model_id.trim().to_ascii_lowercase();
    if kind == crate::providers::types::ProviderKind::Gemini && slug.contains("imagen") {
        return Ok(false);
    }
    Ok(slug.contains("gpt-image")
        || slug.contains("dall-e")
        || slug.contains("imagine")
        || slug.contains("image"))
}

fn generation_event_handler(
    app: AppHandle,
    request: &GenerateImagesRequest,
) -> crate::providers::types::EventHandler {
    let run_id = request
        .client_task_id
        .clone()
        .unwrap_or_else(|| "pending".to_owned());
    std::sync::Arc::new(move |event| {
        let event = provider_event_value(event);
        let _ = app.emit(
            "generation-event",
            GenerationEventEnvelope {
                run_id: run_id.clone(),
                event,
            },
        );
    })
}

fn ensure_client_task_id(request: &mut GenerateImagesRequest) {
    if request
        .client_task_id
        .as_deref()
        .is_none_or(|value| value.trim().is_empty())
    {
        request.client_task_id = Some(uuid::Uuid::new_v4().to_string());
    }
}

fn provider_event_value(event: crate::providers::types::RunEvent) -> Value {
    use crate::providers::types::RunEvent;
    match event {
        RunEvent::Started { request_id } => json!({
            "type": "started",
            "requestId": request_id,
        }),
        RunEvent::PartialImage { index, image, .. } => json!({
            "type": "partial_image",
            "index": index,
            "image": image,
        }),
        RunEvent::TextDelta { text, .. } => json!({
            "type": "text_delta",
            "text": text,
        }),
        RunEvent::Checkpoint {
            event_id, status, ..
        } => json!({
            "type": "checkpoint",
            "eventId": event_id,
            "status": status,
        }),
        RunEvent::Progress { status, .. } => json!({
            "type": "progress",
            "status": status,
        }),
        RunEvent::Completed { response } => json!({
            "type": "completed",
            "requestId": response.request_id,
            "model": response.model,
        }),
    }
}

async fn sync_workspace_projects(
    state: &AppState,
    snapshot: &WorkspaceSnapshot,
) -> CommandResult<()> {
    for project in &snapshot.projects {
        if project.storage_path.trim().is_empty() {
            continue;
        }
        let store = state
            .open_or_create_project(&project.id, &project.name, &project.storage_path)
            .await?;
        let mut summary = store.summary().await?;
        summary.name.clone_from(&project.name);
        summary.updated_at = parse_timestamp(&project.updated_at).unwrap_or_else(Utc::now);
        summary.default_provider_profile_id =
            non_empty(&project.settings.default_provider_id).map(ToOwned::to_owned);
        summary.default_model_id =
            non_empty(&project.settings.default_model).map(ToOwned::to_owned);
        summary.default_parameters = BTreeMap::from([
            (
                "useCommonDescriptions".to_owned(),
                Value::Bool(project.settings.use_common_descriptions),
            ),
            (
                "saveMetadata".to_owned(),
                Value::Bool(project.settings.save_metadata),
            ),
            (
                "saveRawResponse".to_owned(),
                Value::Bool(project.settings.save_raw_response),
            ),
            (
                "autoOpenFolder".to_owned(),
                Value::Bool(project.settings.auto_open_folder),
            ),
            (
                "namingPattern".to_owned(),
                Value::String(project.settings.naming_pattern.clone()),
            ),
        ]);
        store.save_summary(&summary).await?;
        state.global_store().add_recent_project(&summary).await?;

        for (index, description) in project.descriptions.iter().enumerate() {
            let created_at = parse_timestamp(&description.created_at).unwrap_or_else(Utc::now);
            store
                .upsert_prompt_context(&PromptContext {
                    id: description.id.clone(),
                    name: description.title.clone(),
                    content: description.content.clone(),
                    placement: match description.placement {
                        DescriptionPlacement::Prefix => ContextPlacement::Prepend,
                        DescriptionPlacement::Suffix => ContextPlacement::Append,
                    },
                    prefix_content: description.prefix_content.clone(),
                    suffix_content: description.suffix_content.clone(),
                    negative_content: description.negative_content.clone(),
                    sort_order: i32::try_from(index).unwrap_or(i32::MAX),
                    enabled: description.enabled,
                    created_at,
                    updated_at: Utc::now(),
                })
                .await?;
        }

        for preset in &project.presets {
            let created_at = parse_timestamp(&preset.created_at).unwrap_or_else(Utc::now);
            let output = OutputSpec {
                count: 1,
                size: if preset.size.eq_ignore_ascii_case("auto") {
                    ImageSize::Auto
                } else {
                    ImageSize::Preset {
                        value: preset.size.clone(),
                    }
                },
                aspect_ratio: non_empty(&preset.aspect_ratio).map(ToOwned::to_owned),
                quality: non_empty(&preset.quality).map(ToOwned::to_owned),
                ..OutputSpec::default()
            };
            store
                .upsert_preset(&GenerationPreset {
                    id: preset.id.clone(),
                    name: preset.name.clone(),
                    provider_profile_id: non_empty(&preset.provider_id).map(ToOwned::to_owned),
                    model_id: non_empty(&preset.model).map(ToOwned::to_owned),
                    operation: Some(match preset.mode {
                        GenerationMode::Generate => Operation::Generate,
                        GenerationMode::Edit | GenerationMode::Mask => Operation::Edit,
                        GenerationMode::Video => Operation::VideoReferenceToImage,
                        GenerationMode::Variation => Operation::Variation,
                        GenerationMode::ConversationContinue => Operation::ConversationContinue,
                    }),
                    parameters: BTreeMap::from([
                        ("description".to_owned(), json!(preset.description)),
                        ("promptTemplate".to_owned(), json!(preset.prompt_template)),
                        ("outputFormat".to_owned(), json!(preset.output_format)),
                    ]),
                    output: Some(output),
                    created_at,
                    updated_at: Utc::now(),
                })
                .await?;
        }
    }
    Ok(())
}

fn non_empty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

fn mime_for_path(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        _ => "image/png",
    }
}

pub fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new()
        .commands(tauri_specta::collect_commands![
            workspace_load,
            workspace_save,
            diagnostics_report,
            diagnostics_open_logs,
            queue_pause,
            queue_resume,
            queue_status,
            provider_secret_get,
            provider_secret_exists,
            provider_secret_set,
            provider_secret_delete,
            provider_header_secret_set,
            provider_header_secret_exists,
            provider_header_secret_delete,
            provider_delete,
            provider_test,
            provider_sync_models,
            generate_images,
            generate_images_legacy,
            reveal_path,
            export_asset,
            project_create,
            project_open,
            project_load_details,
            project_close,
            projects_open_list,
            project_import_inputs,
            reference_preview,
            project_asset_data_url,
            prompt_context_list,
            prompt_context_upsert,
            prompt_context_delete,
            generation_preset_list,
            generation_preset_upsert,
            generation_preset_delete,
            history_list,
            history_get,
            history_details,
            history_delete,
            history_clear,
            run_cancel,
            remote_tasks_poll,
            project_remap_provider,
        ])
        .typ::<GenerationEventEnvelope>()
}

async fn load_history_details(
    store: &crate::storage::ProjectStore,
    run: RunRecord,
) -> CommandResult<HistoryDetailsDto> {
    let stored_outputs = store.list_outputs(&run.id).await?;
    let mut outputs = Vec::with_capacity(stored_outputs.len());
    for output in stored_outputs {
        let preview = history_thumbnail(store.layout().root(), &output).await?;
        outputs.push(HistoryOutputDto {
            output,
            preview_data_url: preview.as_ref().map(|preview| preview.data_url.clone()),
            width: preview.as_ref().map(|preview| preview.width),
            height: preview.as_ref().map(|preview| preview.height),
        });
    }
    Ok(HistoryDetailsDto {
        usage: store.list_usage(&run.id).await?,
        errors: store.list_errors(&run.id).await?,
        run,
        outputs,
    })
}

struct HistoryThumbnail {
    data_url: String,
    width: u32,
    height: u32,
}

async fn history_thumbnail(
    project_root: &std::path::Path,
    output: &crate::domain::OutputPart,
) -> CommandResult<Option<HistoryThumbnail>> {
    if output.kind != crate::domain::OutputPartKind::Image {
        return Ok(None);
    }
    let Some(local_path) = output.local_path.clone() else {
        return Ok(None);
    };
    let project_root = project_root.to_owned();
    tokio::task::spawn_blocking(move || {
        let Ok(project_root) = project_root
            .canonicalize()
            .map(strip_extended_length_prefix)
        else {
            return Ok(None);
        };
        let candidate = if local_path.is_absolute() {
            local_path
        } else {
            project_root.join(&local_path)
        };
        let Ok(path) = candidate.canonicalize().map(strip_extended_length_prefix) else {
            return Ok(None);
        };
        if !path.starts_with(&project_root) || !path.is_file() {
            return Ok(None);
        }
        let Ok(metadata) = std::fs::metadata(&path) else {
            return Ok(None);
        };
        if metadata.len() > 50 * 1024 * 1024 {
            return Ok(None);
        }
        let Ok((width, height)) = image::image_dimensions(&path) else {
            return Ok(None);
        };
        if u64::from(width).saturating_mul(u64::from(height)) > 100_000_000 {
            return Ok(None);
        }
        let Ok(image) = image::open(&path) else {
            return Ok(None);
        };
        let thumbnail = image.thumbnail(512, 512);
        let mut bytes = std::io::Cursor::new(Vec::new());
        thumbnail
            .write_to(&mut bytes, image::ImageFormat::Png)
            .map_err(|error| CommandError::new("preview_encode", error.to_string()))?;
        Ok(Some(HistoryThumbnail {
            data_url: format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())
            ),
            width,
            height,
        }))
    })
    .await
    .map_err(|error| CommandError::new("preview_task", error.to_string()))?
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::Arc;

    use crate::bridge::{
        ApiMode, CommonDescriptionDto, FrontendAuthScheme, FrontendGenerationPreset,
        FrontendProviderKind, FrontendTaskStatus, GeneratedAsset, GenerationTaskDto,
        HistoryRecordDto, Locale, ProjectDto, ProjectSettingsDto, ProviderProfileDto, UsageDto,
    };
    use crate::domain::{
        ErrorRecord, ExecutionMode, ModelCapability, OutputPart, OutputPartKind,
        ProviderError as DomainProviderError, ProviderKind, RunStatus, UsageRecord,
    };
    use crate::security::MemoryKeyring;
    use crate::storage::GlobalStore;

    use super::*;

    fn project_dto(project_root: &std::path::Path, now: chrono::DateTime<Utc>) -> ProjectDto {
        ProjectDto {
            id: "project".to_owned(),
            name: "Project".to_owned(),
            description: String::new(),
            storage_path: project_root.to_string_lossy().into_owned(),
            created_at: now.to_rfc3339(),
            updated_at: now.to_rfc3339(),
            color: "#000000".to_owned(),
            descriptions: vec![],
            presets: vec![],
            settings: ProjectSettingsDto {
                use_common_descriptions: true,
                save_metadata: true,
                save_raw_response: true,
                auto_open_folder: false,
                naming_pattern: "{run-id}".to_owned(),
                default_provider_id: String::new(),
                default_model: String::new(),
                flat_output: false,
                default_stream: None,
            },
        }
    }

    fn provider_dto() -> ProviderProfileDto {
        ProviderProfileDto {
            id: "provider".to_owned(),
            name: "OpenAI".to_owned(),
            kind: FrontendProviderKind::Openai,
            base_url: "https://api.openai.com/v1".to_owned(),
            api_key: "sk-command-test-secret".to_owned(),
            has_stored_secret: None,
            api_mode: ApiMode::Native,
            enabled: true,
            models: vec!["gpt-image-1".to_owned()],
            discovered_models: vec![],
            api_version: None,
            organization: None,
            project_id: None,
            custom_header: None,
            last_synced_at: None,
            timeout_seconds: None,
            timeout_ms: None,
            proxy_url: None,
            auth_scheme: Some(FrontendAuthScheme::Bearer),
            auth_header_name: None,
            auth_prefix: None,
            auth_query_name: None,
            custom_headers: vec![],
            models_path: None,
            compatibility_json: None,
            capability_overrides_json: None,
            default_stream: None,
        }
    }

    #[tokio::test]
    async fn workspace_sync_never_deletes_portable_project_content() {
        let directory = tempfile::tempdir().unwrap();
        let global = GlobalStore::open(directory.path().join("global.sqlite3"))
            .await
            .unwrap();
        let state = AppState::new(global, Arc::new(MemoryKeyring::default()));
        let project_root = directory.path().join("project");
        let store = state
            .create_project("project", "Project", &project_root)
            .await
            .unwrap();
        let now = Utc::now();
        store
            .upsert_prompt_context(&PromptContext {
                id: "context".to_owned(),
                name: "Context".to_owned(),
                content: "anime".to_owned(),
                placement: ContextPlacement::Prepend,
                prefix_content: "anime".to_owned(),
                suffix_content: String::new(),
                negative_content: String::new(),
                sort_order: 0,
                enabled: true,
                created_at: now,
                updated_at: now,
            })
            .await
            .unwrap();
        store
            .upsert_preset(&GenerationPreset {
                id: "preset".to_owned(),
                name: "Preset".to_owned(),
                provider_profile_id: None,
                model_id: None,
                operation: Some(Operation::Generate),
                parameters: BTreeMap::new(),
                output: None,
                created_at: now,
                updated_at: now,
            })
            .await
            .unwrap();
        let snapshot = WorkspaceSnapshot {
            locale: Locale::EnUs,
            active_project_id: "project".to_owned(),
            projects: vec![project_dto(&project_root, now)],
            providers: vec![],
            history: vec![],
        };

        sync_workspace_projects(&state, &snapshot).await.unwrap();

        assert_eq!(store.list_prompt_contexts().await.unwrap().len(), 1);
        assert_eq!(store.list_presets().await.unwrap().len(), 1);
    }

    #[test]
    fn model_sync_filter_keeps_only_image_generation_slugs() {
        let openai = crate::providers::types::ProviderKind::OpenAi;
        assert!(is_image_model_slug(openai, "gpt-image-1").unwrap());
        assert!(is_image_model_slug(openai, "dall-e-3").unwrap());
        assert!(is_image_model_slug(openai, "vendor-image-experimental").unwrap());
        assert!(!is_image_model_slug(openai, "gpt-4o").unwrap());
        assert!(!is_image_model_slug(openai, "tts-1").unwrap());

        let gemini = crate::providers::types::ProviderKind::Gemini;
        assert!(is_image_model_slug(gemini, "gemini-next-image-exp").unwrap());
        assert!(!is_image_model_slug(gemini, "gemini-2.5-flash").unwrap());
        assert!(!is_image_model_slug(gemini, "imagen-4.0-generate").unwrap());
    }

    #[tokio::test]
    async fn global_workspace_excludes_project_payloads_and_credentials() {
        let directory = tempfile::tempdir().unwrap();
        let global = GlobalStore::open(directory.path().join("global.sqlite3"))
            .await
            .unwrap();
        let state = AppState::new(global, Arc::new(MemoryKeyring::default()));
        let project_root = directory.path().join("project");
        let now = Utc::now();
        let mut project = project_dto(&project_root, now);
        project.descriptions.push(CommonDescriptionDto {
            id: "description".to_owned(),
            title: "Reference".to_owned(),
            content: "data:image/png;base64,AAAA".to_owned(),
            enabled: true,
            placement: DescriptionPlacement::Prefix,
            prefix_content: "data:image/png;base64,AAAA".to_owned(),
            suffix_content: String::new(),
            negative_content: String::new(),
            created_at: now.to_rfc3339(),
        });
        project.presets.push(FrontendGenerationPreset {
            id: "preset".to_owned(),
            name: "Preset".to_owned(),
            description: "large project-only value".to_owned(),
            provider_id: "provider".to_owned(),
            model: "gpt-image-1".to_owned(),
            mode: GenerationMode::Generate,
            aspect_ratio: "auto".to_owned(),
            size: "auto".to_owned(),
            quality: "auto".to_owned(),
            output_format: "png".to_owned(),
            response_format: None,
            prompt_template: String::new(),
            created_at: now.to_rfc3339(),
        });
        let history = HistoryRecordDto {
            task: GenerationTaskDto {
                id: "run".to_owned(),
                project_id: "project".to_owned(),
                provider_id: "provider".to_owned(),
                provider_name: "OpenAI".to_owned(),
                model: "gpt-image-1".to_owned(),
                prompt: "draw".to_owned(),
                composed_prompt: "draw".to_owned(),
                mode: GenerationMode::Generate,
                status: FrontendTaskStatus::Completed,
                progress: 100.0,
                count: 1,
                created_at: now.to_rfc3339(),
                duration_ms: None,
                error: None,
                assets: vec![GeneratedAsset {
                    id: "asset".to_owned(),
                    task_id: "run".to_owned(),
                    url: "data:image/png;base64,BBBB".to_owned(),
                    file_path: "assets/outputs/result.png".to_owned(),
                    width: 1,
                    height: 1,
                    format: "png".to_owned(),
                    prompt: "draw".to_owned(),
                    created_at: now.to_rfc3339(),
                    selected: None,
                }],
                response_parts: vec![],
                request_id: None,
                interaction_id: None,
                usage: Some(UsageDto::default()),
            },
            favorite: false,
        };
        let snapshot = WorkspaceSnapshot {
            locale: Locale::EnUs,
            active_project_id: "project".to_owned(),
            projects: vec![project],
            providers: vec![provider_dto()],
            history: vec![history],
        };

        save_workspace_snapshot(&state, snapshot).await.unwrap();

        let raw: String =
            sqlx::query_scalar("SELECT value_json FROM app_settings WHERE key = 'workspace.v1'")
                .fetch_one(state.global_store().pool())
                .await
                .unwrap();
        assert!(!raw.contains("data:image"));
        assert!(!raw.contains("sk-command-test-secret"));
        let stored: WorkspaceSnapshot = serde_json::from_str(&raw).unwrap();
        assert!(stored.history.is_empty());
        assert!(stored.projects[0].descriptions.is_empty());
        assert!(stored.projects[0].presets.is_empty());

        assert!(delete_provider_data(&state, "provider").await.unwrap());
        assert!(
            state
                .global_store()
                .provider("provider")
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            state
                .secret(crate::security::CredentialKey::provider("provider"))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn history_details_restore_outputs_usage_errors_and_bounded_preview() {
        let directory = tempfile::tempdir().unwrap();
        let global = GlobalStore::open(directory.path().join("global.sqlite3"))
            .await
            .unwrap();
        let state = AppState::new(global, Arc::new(MemoryKeyring::default()));
        let project_root = directory.path().join("project");
        let store = state
            .create_project("project", "Project", &project_root)
            .await
            .unwrap();
        let now = Utc::now();
        let mut capability = ModelCapability::generic(ProviderKind::OpenAi, "gpt-image-1");
        capability.operations = BTreeSet::from([Operation::Generate]);
        capability.execution_modes = BTreeSet::from([ExecutionMode::Realtime]);
        let run = RunRecord {
            id: "run".to_owned(),
            group_id: None,
            request: crate::domain::GenerationRequest::new(
                "project",
                "provider",
                "gpt-image-1",
                "draw",
            ),
            status: RunStatus::Succeeded,
            raw_prompt: "draw".to_owned(),
            final_prompt: "draw".to_owned(),
            context_snapshot: vec![],
            preset_snapshot: None,
            capability_snapshot: capability,
            capability_registry_version: "2026-07-12".to_owned(),
            model_version: None,
            provider_request_id: Some("request".to_owned()),
            redacted_request: None,
            redacted_response: None,
            started_at: Some(now),
            finished_at: Some(now),
            created_at: now,
            updated_at: now,
        };
        store.upsert_run(&run).await.unwrap();
        let output_path = project_root.join("assets/outputs/history.png");
        std::fs::create_dir_all(output_path.parent().unwrap()).unwrap();
        image::DynamicImage::ImageRgb8(image::RgbImage::new(640, 320))
            .save(&output_path)
            .unwrap();
        store
            .upsert_output(&OutputPart {
                id: "output".to_owned(),
                run_id: run.id.clone(),
                job_id: None,
                sequence: 0,
                kind: OutputPartKind::Image,
                text: None,
                local_path: Some(std::path::PathBuf::from("assets/outputs/history.png")),
                remote_url: None,
                provider_file_id: None,
                mime_type: Some("image/png".to_owned()),
                sha256: None,
                size_bytes: None,
                metadata: BTreeMap::new(),
                created_at: now,
            })
            .await
            .unwrap();
        store
            .save_usage(&UsageRecord {
                id: "usage".to_owned(),
                run_id: run.id.clone(),
                job_id: None,
                input_tokens: Some(10),
                output_tokens: Some(20),
                image_count: Some(1),
                input_bytes: None,
                output_bytes: None,
                cost_micros: None,
                currency: None,
                details: BTreeMap::new(),
                created_at: now,
            })
            .await
            .unwrap();
        store
            .save_error(&ErrorRecord {
                id: "error".to_owned(),
                run_id: run.id.clone(),
                job_id: None,
                error: DomainProviderError {
                    code: "partial".to_owned(),
                    message: "one part failed".to_owned(),
                    http_status: None,
                    retryable: false,
                    request_id: None,
                    provider: Some(ProviderKind::OpenAi),
                    details: None,
                },
                created_at: now,
            })
            .await
            .unwrap();

        let details = load_history_details(&store, run).await.unwrap();

        assert_eq!(details.outputs.len(), 1);
        assert_eq!(details.usage.len(), 1);
        assert_eq!(details.errors.len(), 1);
        assert_eq!(details.outputs[0].width, Some(640));
        assert_eq!(details.outputs[0].height, Some(320));
        assert!(
            details.outputs[0]
                .preview_data_url
                .as_deref()
                .is_some_and(|url| url.starts_with("data:image/png;base64,"))
        );
    }

    #[test]
    fn reads_latest_diagnostic_log_tail() {
        let directory = tempfile::tempdir().unwrap();
        let log_path = directory.path().join("imageworkbench.log.2026-07-17");
        std::fs::write(&log_path, "first line\nproject open failed\n").unwrap();

        let (files, tail) = read_diagnostic_logs(directory.path());

        assert_eq!(files, vec!["imageworkbench.log.2026-07-17"]);
        assert!(tail.contains("project open failed"));
    }
}
