use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;

use super::{
    ProjectLayout, ProjectStore, StorageError, StorageResult, sha256_file,
    strip_extended_length_prefix,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectDuplicateMode {
    Full,
    Configuration,
}

pub async fn duplicate_project(
    source: &ProjectStore,
    destination: impl AsRef<Path>,
    project_id: &str,
    name: &str,
    mode: ProjectDuplicateMode,
) -> StorageResult<ProjectStore> {
    let destination = validate_empty_destination(source.layout().root(), destination.as_ref())?;
    let duplicated = match mode {
        ProjectDuplicateMode::Full => {
            let destination_layout = ProjectLayout::create(&destination)?;
            source
                .snapshot_database(destination_layout.database_path())
                .await?;
            copy_directory_contents(
                &source.layout().input_directory(),
                &destination_layout.input_directory(),
            )?;
            copy_directory_contents(
                &source.layout().output_directory(),
                &destination_layout.output_directory(),
            )?;
            copy_directory_contents(
                &source.layout().preview_directory(),
                &destination_layout.preview_directory(),
            )?;
            let store = ProjectStore::open(&destination).await?;
            store.mark_local_work_interrupted().await?;
            store.detach_remote_lifecycle().await?;
            store.remap_project_id(project_id).await?;
            store
        }
        ProjectDuplicateMode::Configuration => {
            let store = ProjectStore::create(&destination, name).await?;
            for context in source.list_prompt_contexts().await? {
                store.upsert_prompt_context(&context).await?;
            }
            for preset in source.list_presets().await? {
                store.upsert_preset(&preset).await?;
            }
            store
        }
    };

    let source_summary = source.summary().await?;
    let now = Utc::now();
    let mut summary = duplicated.summary().await?;
    summary.id = project_id.to_owned();
    summary.name = name.trim().to_owned();
    summary.created_at = now;
    summary.updated_at = now;
    summary.last_opened_at = now;
    summary.default_provider_profile_id = source_summary.default_provider_profile_id;
    summary.default_model_id = source_summary.default_model_id;
    summary.default_parameters = source_summary.default_parameters;
    duplicated.save_summary(&summary).await?;
    Ok(duplicated)
}

pub async fn stage_project_move(
    source: &ProjectStore,
    destination: impl AsRef<Path>,
) -> StorageResult<ProjectStore> {
    let destination = validate_empty_destination(source.layout().root(), destination.as_ref())?;
    let parent = destination
        .parent()
        .ok_or_else(|| StorageError::InvalidPath(destination.clone()))?;
    let file_name = destination
        .file_name()
        .ok_or_else(|| StorageError::InvalidPath(destination.clone()))?
        .to_string_lossy();
    let staging = parent.join(format!(
        ".{file_name}.imageworkbench-moving-{}",
        uuid::Uuid::new_v4()
    ));
    let result = async {
        let staging_layout = ProjectLayout::create(&staging)?;
        source
            .snapshot_database(staging_layout.database_path())
            .await?;
        for (from, to) in [
            (
                source.layout().input_directory(),
                staging_layout.input_directory(),
            ),
            (
                source.layout().output_directory(),
                staging_layout.output_directory(),
            ),
            (
                source.layout().preview_directory(),
                staging_layout.preview_directory(),
            ),
        ] {
            copy_directory_contents(&from, &to)?;
            verify_directory_copy(&from, &to)?;
        }
        let staged = ProjectStore::open(&staging).await?;
        let source_id = source.summary().await?.id;
        let staged_id = staged.summary().await?.id;
        staged.pool().close().await;
        drop(staged);
        if source_id != staged_id {
            return Err(StorageError::InvalidProject(
                "staged project identity does not match the source".to_owned(),
            ));
        }
        if destination.exists() {
            fs::remove_dir(&destination)?;
        }
        // SQLite may briefly retain a handle on Windows even after the
        // staging connection is closed. Prefer the atomic rename, but use a
        // verified directory copy as a safe fallback rather than surfacing a
        // platform-specific "access denied" move failure.
        if let Err(rename_error) = fs::rename(&staging, &destination) {
            if rename_error.kind() != std::io::ErrorKind::PermissionDenied {
                return Err(rename_error.into());
            }
            fs::create_dir_all(&destination)?;
            copy_directory_contents(&staging, &destination)?;
            for relative in ["assets/inputs", "assets/outputs", "assets/previews"] {
                verify_directory_copy(&staging.join(relative), &destination.join(relative))?;
            }
            fs::remove_dir_all(&staging)?;
        }
        ProjectStore::open(&destination).await
    }
    .await;
    if result.is_err() && staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

pub fn delete_owned_project_files(root: impl AsRef<Path>) -> StorageResult<()> {
    let root = strip_extended_length_prefix(root.as_ref().canonicalize()?);
    let layout = ProjectLayout::open(&root)?;
    for path in [
        layout.internal_directory(),
        layout.input_directory(),
        layout.output_directory(),
        layout.preview_directory(),
    ] {
        remove_owned_directory(&root, &path)?;
    }
    remove_if_empty(&root.join("assets"))?;
    remove_if_empty(&root)?;
    Ok(())
}

fn validate_empty_destination(source: &Path, destination: &Path) -> StorageResult<PathBuf> {
    if destination.as_os_str().is_empty() {
        return Err(StorageError::InvalidPath(destination.to_owned()));
    }
    let source = strip_extended_length_prefix(source.canonicalize()?);
    if destination.exists() {
        let destination = strip_extended_length_prefix(destination.canonicalize()?);
        if destination == source
            || destination.starts_with(&source)
            || source.starts_with(&destination)
        {
            return Err(StorageError::Conflict(
                "the copy destination must be separate from the source project".to_owned(),
            ));
        }
        if !destination.is_dir() || fs::read_dir(&destination)?.next().is_some() {
            return Err(StorageError::Conflict(
                "the copy destination directory must be empty".to_owned(),
            ));
        }
        Ok(destination)
    } else {
        let parent = destination
            .parent()
            .ok_or_else(|| StorageError::InvalidPath(destination.to_owned()))?;
        let parent = strip_extended_length_prefix(parent.canonicalize()?);
        let destination = parent.join(
            destination
                .file_name()
                .ok_or_else(|| StorageError::InvalidPath(destination.to_owned()))?,
        );
        if destination == source
            || destination.starts_with(&source)
            || source.starts_with(&destination)
        {
            return Err(StorageError::Conflict(
                "the copy destination must be separate from the source project".to_owned(),
            ));
        }
        Ok(destination)
    }
}

fn copy_directory_contents(source: &Path, destination: &Path) -> StorageResult<()> {
    if !source.exists() {
        return Ok(());
    }
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target = destination.join(entry.file_name());
        if file_type.is_symlink() {
            return Err(StorageError::InvalidPath(entry.path()));
        }
        if file_type.is_dir() {
            copy_directory_contents(&entry.path(), &target)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn verify_directory_copy(source: &Path, destination: &Path) -> StorageResult<()> {
    let source_manifest = directory_manifest(source, source)?;
    let destination_manifest = directory_manifest(destination, destination)?;
    if source_manifest != destination_manifest {
        return Err(StorageError::Conflict(format!(
            "copied project assets failed verification for {}",
            source.display()
        )));
    }
    Ok(())
}

fn directory_manifest(
    root: &Path,
    directory: &Path,
) -> StorageResult<BTreeMap<PathBuf, (u64, String)>> {
    let mut manifest = BTreeMap::new();
    if !directory.exists() {
        return Ok(manifest);
    }
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(StorageError::InvalidPath(entry.path()));
        }
        if file_type.is_dir() {
            manifest.extend(directory_manifest(root, &entry.path())?);
        } else if file_type.is_file() {
            let relative = entry
                .path()
                .strip_prefix(root)
                .map_err(|_| StorageError::InvalidPath(entry.path()))?
                .to_owned();
            let size = entry.metadata()?.len();
            manifest.insert(relative, (size, sha256_file(entry.path())?));
        }
    }
    Ok(manifest)
}

fn remove_owned_directory(root: &Path, path: &Path) -> StorageResult<()> {
    if !path.exists() {
        return Ok(());
    }
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(StorageError::InvalidPath(path.to_owned()));
    }
    let canonical = strip_extended_length_prefix(path.canonicalize()?);
    if !canonical.starts_with(root) || canonical == root {
        return Err(StorageError::InvalidPath(canonical));
    }
    remove_dir_all_with_retry(&canonical)?;
    Ok(())
}

fn remove_dir_all_with_retry(path: &Path) -> StorageResult<()> {
    let mut last_error = None;
    for attempt in 0..8 {
        match fs::remove_dir_all(path) {
            Ok(()) => return Ok(()),
            Err(error)
                if error.kind() == std::io::ErrorKind::PermissionDenied
                    || error.raw_os_error() == Some(32) =>
            {
                last_error = Some(error);
                // Windows can hold a recently closed SQLite/WAL handle for a
                // short period (and antivirus scanners may briefly inspect
                // the directory). Give the OS a bounded grace period before
                // reporting the deletion failure.
                if attempt < 7 {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
            Err(error) => return Err(error.into()),
        }
    }
    Err(last_error
        .expect("retry loop must retain the last deletion error")
        .into())
}

fn remove_if_empty(path: &Path) -> StorageResult<()> {
    if path.is_dir() && fs::read_dir(path)?.next().is_none() {
        fs::remove_dir(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::domain::{
        ContextPlacement, GenerationPreset, ModelCapability, OutputPart, OutputPartKind,
        PromptContext, ProviderKind, RunRecord, RunStatus,
    };
    use crate::storage::RemoteFileRecord;

    use super::*;

    #[tokio::test]
    async fn configuration_copy_keeps_settings_contexts_and_presets_without_assets() {
        let directory = tempfile::tempdir().unwrap();
        let source = ProjectStore::create(directory.path().join("source"), "Source")
            .await
            .unwrap();
        let mut summary = source.summary().await.unwrap();
        summary.default_model_id = Some("gpt-image-1".to_owned());
        summary
            .default_parameters
            .insert("color".to_owned(), "#123456".into());
        source.save_summary(&summary).await.unwrap();
        source
            .upsert_prompt_context(&PromptContext {
                id: "context".to_owned(),
                name: "Context".to_owned(),
                content: "prefix".to_owned(),
                placement: ContextPlacement::Prepend,
                prefix_content: "prefix".to_owned(),
                suffix_content: String::new(),
                negative_content: String::new(),
                sort_order: 0,
                enabled: true,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            })
            .await
            .unwrap();
        source
            .upsert_preset(&GenerationPreset {
                id: "preset".to_owned(),
                name: "Preset".to_owned(),
                provider_profile_id: None,
                model_id: Some("gpt-image-1".to_owned()),
                operation: None,
                parameters: BTreeMap::new(),
                output: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            })
            .await
            .unwrap();
        fs::write(
            source.layout().input_directory().join("source.png"),
            b"image",
        )
        .unwrap();

        let copy = duplicate_project(
            &source,
            directory.path().join("configuration-copy"),
            "copy-id",
            "Copy",
            ProjectDuplicateMode::Configuration,
        )
        .await
        .unwrap();

        assert_eq!(copy.summary().await.unwrap().id, "copy-id");
        assert_eq!(
            copy.summary().await.unwrap().default_model_id.as_deref(),
            Some("gpt-image-1")
        );
        assert_eq!(copy.list_prompt_contexts().await.unwrap().len(), 1);
        assert_eq!(copy.list_presets().await.unwrap().len(), 1);
        assert!(
            fs::read_dir(copy.layout().input_directory())
                .unwrap()
                .next()
                .is_none()
        );
    }

    #[tokio::test]
    async fn full_copy_keeps_assets_and_database_records() {
        let directory = tempfile::tempdir().unwrap();
        let source = ProjectStore::create(directory.path().join("source"), "Source")
            .await
            .unwrap();
        source
            .upsert_prompt_context(&PromptContext::new("Context", "content"))
            .await
            .unwrap();
        let source_id = source.summary().await.unwrap().id;
        let mut request =
            crate::domain::GenerationRequest::new(&source_id, "provider", "gpt-image-1", "draw");
        request.metadata.insert(
            "continuationId".to_owned(),
            serde_json::Value::String("resp_original".to_owned()),
        );
        source
            .upsert_run(&RunRecord {
                id: "run".to_owned(),
                group_id: None,
                request,
                status: RunStatus::Succeeded,
                raw_prompt: "draw".to_owned(),
                final_prompt: "draw".to_owned(),
                context_snapshot: vec![],
                preset_snapshot: None,
                capability_snapshot: ModelCapability::generic(ProviderKind::OpenAi, "gpt-image-1"),
                capability_registry_version: "test".to_owned(),
                model_version: None,
                provider_request_id: Some("resp_original".to_owned()),
                redacted_request: None,
                redacted_response: None,
                started_at: None,
                finished_at: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            })
            .await
            .unwrap();
        source
            .upsert_output(&OutputPart {
                id: "output".to_owned(),
                run_id: "run".to_owned(),
                job_id: None,
                sequence: 0,
                kind: OutputPartKind::Image,
                text: None,
                local_path: Some(PathBuf::from("assets/outputs/result.png")),
                remote_url: Some("https://provider.example/result.png".to_owned()),
                provider_file_id: Some("remote-file".to_owned()),
                mime_type: Some("image/png".to_owned()),
                sha256: None,
                size_bytes: None,
                metadata: BTreeMap::new(),
                created_at: Utc::now(),
            })
            .await
            .unwrap();
        source
            .upsert_remote_file(&RemoteFileRecord {
                id: "remote-record".to_owned(),
                provider_profile_id: "provider".to_owned(),
                provider_file_id: "remote-file".to_owned(),
                purpose: None,
                expires_at: None,
                metadata: BTreeMap::new(),
                created_at: Utc::now(),
            })
            .await
            .unwrap();
        fs::write(
            source.layout().output_directory().join("result.png"),
            b"image",
        )
        .unwrap();

        let copy = duplicate_project(
            &source,
            directory.path().join("full-copy"),
            "copy-id",
            "Full copy",
            ProjectDuplicateMode::Full,
        )
        .await
        .unwrap();

        assert_eq!(copy.list_prompt_contexts().await.unwrap().len(), 1);
        assert!(
            copy.layout()
                .output_directory()
                .join("result.png")
                .is_file()
        );
        let copied_run = copy.list_runs(10, 0).await.unwrap().remove(0);
        assert_eq!(copied_run.request.project_id, "copy-id");
        assert!(copied_run.provider_request_id.is_none());
        assert!(!copied_run.request.metadata.contains_key("continuationId"));
        let copied_output = copy.output("output").await.unwrap().unwrap();
        assert!(copied_output.remote_url.is_none());
        assert!(copied_output.provider_file_id.is_none());
        assert!(copy.list_remote_files("provider").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn staged_move_verifies_assets_and_preserves_project_identity() {
        let directory = tempfile::tempdir().unwrap();
        let source = ProjectStore::create(directory.path().join("source"), "Source")
            .await
            .unwrap();
        let source_id = source.summary().await.unwrap().id;
        fs::write(
            source.layout().input_directory().join("reference.png"),
            b"reference-image",
        )
        .unwrap();
        fs::write(
            source.layout().output_directory().join("result.png"),
            b"generated-image",
        )
        .unwrap();
        let destination = directory.path().join("destination");

        let moved = stage_project_move(&source, &destination).await.unwrap();

        assert_eq!(moved.summary().await.unwrap().id, source_id);
        assert_eq!(
            fs::read(moved.layout().input_directory().join("reference.png")).unwrap(),
            b"reference-image"
        );
        assert_eq!(
            fs::read(moved.layout().output_directory().join("result.png")).unwrap(),
            b"generated-image"
        );
        assert!(source.layout().database_path().is_file());
    }

    #[tokio::test]
    async fn deletion_removes_only_owned_project_directories() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("mixed-folder");
        let store = ProjectStore::create(&root, "Project").await.unwrap();
        fs::write(root.join("user-notes.txt"), b"keep").unwrap();
        store.pool().close().await;
        drop(store);

        delete_owned_project_files(&root).unwrap();

        assert!(root.join("user-notes.txt").is_file());
        assert!(!root.join(".imageworkbench").exists());
        assert!(!root.join("assets").exists());
    }
}
