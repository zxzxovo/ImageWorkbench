use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{StorageError, StorageResult};

const INTERNAL_DIRECTORY: &str = ".imageworkbench";

/// Strips the `\\?\` extended-length path prefix that Rust's `canonicalize()`
/// adds on Windows, returning a plain Win32 path usable by all APIs and
/// storable without confusing the frontend path-join logic.
pub fn strip_extended_length_prefix(path: PathBuf) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let s = path.to_string_lossy();
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            return PathBuf::from(stripped.to_owned());
        }
    }
    path
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectLayout {
    root: PathBuf,
}

impl ProjectLayout {
    pub fn create(root: impl AsRef<Path>) -> StorageResult<Self> {
        let root = root.as_ref();
        if root.exists() && !root.is_dir() {
            return Err(StorageError::InvalidProject(format!(
                "{} is not a directory",
                root.display()
            )));
        }
        fs::create_dir_all(root)?;
        let root = strip_extended_length_prefix(root.canonicalize()?);
        let layout = Self { root };
        layout.ensure_directories()?;
        Ok(layout)
    }

    pub fn open(root: impl AsRef<Path>) -> StorageResult<Self> {
        let root = strip_extended_length_prefix(root.as_ref().canonicalize()?);
        if !root.is_dir() {
            return Err(StorageError::InvalidProject(format!(
                "{} is not a directory",
                root.display()
            )));
        }
        let layout = Self { root };
        if !layout.database_path().is_file() {
            return Err(StorageError::InvalidProject(
                "the project database is missing".to_owned(),
            ));
        }
        layout.ensure_directories()?;
        Ok(layout)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn internal_directory(&self) -> PathBuf {
        self.root.join(INTERNAL_DIRECTORY)
    }

    pub fn database_path(&self) -> PathBuf {
        self.internal_directory().join("project.sqlite3")
    }

    pub fn input_directory(&self) -> PathBuf {
        self.root.join("assets").join("inputs")
    }

    pub fn output_directory(&self) -> PathBuf {
        self.root.join("assets").join("outputs")
    }

    pub fn preview_directory(&self) -> PathBuf {
        self.root.join("assets").join("previews")
    }

    pub fn output_run_directory(&self, date: NaiveDate, run_id: &str) -> StorageResult<PathBuf> {
        validate_filename(run_id)?;
        let path = self
            .output_directory()
            .join(date.format("%Y-%m-%d").to_string())
            .join(run_id);
        fs::create_dir_all(&path)?;
        Ok(path)
    }

    pub fn resolve_relative(&self, relative: impl AsRef<Path>) -> StorageResult<PathBuf> {
        let relative = relative.as_ref();
        if relative.as_os_str().is_empty()
            || relative.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(StorageError::InvalidPath(relative.to_owned()));
        }
        Ok(self.root.join(relative))
    }

    fn ensure_directories(&self) -> StorageResult<()> {
        for path in [
            self.internal_directory(),
            self.input_directory(),
            self.output_directory(),
            self.preview_directory(),
        ] {
            fs::create_dir_all(path)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StoredFile {
    pub path: PathBuf,
    pub sha256: String,
    pub size_bytes: u64,
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn sha256_file(path: impl AsRef<Path>) -> StorageResult<String> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

/// Writes a new file through a sibling temporary file and an atomic rename.
/// Existing destinations are rejected so Windows and Unix have identical rules.
pub fn atomic_write_new(path: impl AsRef<Path>, bytes: &[u8]) -> StorageResult<StoredFile> {
    let path = path.as_ref();
    if path.exists() {
        return Err(StorageError::Conflict(format!(
            "{} already exists",
            path.display()
        )));
    }
    let parent = path
        .parent()
        .ok_or_else(|| StorageError::InvalidPath(path.to_owned()))?;
    fs::create_dir_all(parent)?;
    let temp_path = temporary_sibling(path);
    let result = (|| -> StorageResult<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temp_path, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result?;
    Ok(StoredFile {
        path: path.to_owned(),
        sha256: sha256_bytes(bytes),
        size_bytes: bytes.len() as u64,
    })
}

pub fn import_input_file(
    layout: &ProjectLayout,
    source: impl AsRef<Path>,
) -> StorageResult<StoredFile> {
    let source = source.as_ref();
    let mut input = File::open(source)?;
    let temporary = layout
        .input_directory()
        .join(format!(".import-{}.tmp", Uuid::new_v4()));
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let mut digest = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    let copy_result = (|| -> StorageResult<()> {
        loop {
            let read = input.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            output.write_all(&buffer[..read])?;
            digest.update(&buffer[..read]);
            total += read as u64;
        }
        output.sync_all()?;
        Ok(())
    })();
    if let Err(error) = copy_result {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }

    let hash = format!("{:x}", digest.finalize());
    let extension = source
        .extension()
        .and_then(OsStr::to_str)
        .filter(|value| is_safe_extension(value));
    let filename = match extension {
        Some(extension) => format!("{hash}.{extension}"),
        None => hash.clone(),
    };
    let destination = layout.input_directory().join(filename);
    if destination.exists() {
        fs::remove_file(&temporary)?;
    } else if let Err(error) = fs::rename(&temporary, &destination) {
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
    Ok(StoredFile {
        path: destination,
        sha256: hash,
        size_bytes: total,
    })
}

fn temporary_sibling(path: &Path) -> PathBuf {
    let filename = path.file_name().and_then(OsStr::to_str).unwrap_or("asset");
    path.with_file_name(format!(".{filename}.{}.tmp", Uuid::new_v4()))
}

fn validate_filename(value: &str) -> StorageResult<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_.".contains(character))
    {
        return Err(StorageError::InvalidPath(PathBuf::from(value)));
    }
    Ok(())
}

fn is_safe_extension(extension: &str) -> bool {
    !extension.is_empty()
        && extension.len() <= 10
        && extension
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("imageworkbench-{name}-{}", Uuid::new_v4()))
    }

    #[test]
    fn creates_portable_project_layout_and_rejects_traversal() {
        let root = test_root("layout");
        let layout = ProjectLayout::create(&root).unwrap();

        assert!(layout.input_directory().is_dir());
        assert!(layout.output_directory().is_dir());
        assert!(layout.resolve_relative("assets/inputs/a.png").is_ok());
        assert!(layout.resolve_relative("../outside").is_err());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn writes_new_files_atomically_and_hashes_them() {
        let root = test_root("atomic");
        let path = root.join("output.png");
        let stored = atomic_write_new(&path, b"image bytes").unwrap();

        assert_eq!(stored.sha256, sha256_file(&path).unwrap());
        assert_eq!(stored.size_bytes, 11);
        assert!(atomic_write_new(&path, b"replace").is_err());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn imported_inputs_are_content_addressed_and_deduplicated() {
        let root = test_root("import");
        let layout = ProjectLayout::create(&root).unwrap();
        let source = root.join("source.png");
        fs::write(&source, b"same image").unwrap();

        let first = import_input_file(&layout, &source).unwrap();
        let second = import_input_file(&layout, &source).unwrap();

        assert_eq!(first, second);
        assert!(first.path.is_file());
        fs::remove_dir_all(root).unwrap();
    }
}
