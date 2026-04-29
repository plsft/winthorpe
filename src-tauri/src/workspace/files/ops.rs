//! File-tree mutations: create / rename / delete file or directory.
//!
//! Backs the FileTree right-click context menu. Each op resolves the
//! target through `resolve_allowed_path` so an attacker-supplied path
//! can't escape the workspace root (defense in depth — the IPC layer
//! also gates on workspace identity).
//!
//! Errors surface to the frontend via the standard CommandError chain;
//! the UI shows them as toasts and refreshes the tree.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use super::support::{atomic_write_file, resolve_allowed_path};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateFileResponse {
    pub absolute_path: String,
}

/// Create an empty file at `path`. Errors if it already exists. Creates
/// any missing parent directories along the way (mkdir -p semantics).
pub fn create_file(path: &str) -> Result<CreateFileResponse> {
    let resolved = resolve_allowed_path(Path::new(path), false)?;
    if resolved.exists() {
        bail!(
            "A file or directory already exists at {}",
            resolved.display()
        );
    }
    if let Some(parent) = resolved.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create parent directory {}", parent.display()))?;
    }
    atomic_write_file(&resolved, b"")?;
    Ok(CreateFileResponse {
        absolute_path: resolved.display().to_string(),
    })
}

/// Create a directory at `path`. Errors if it already exists. Creates
/// any missing intermediate directories (mkdir -p semantics).
pub fn create_directory(path: &str) -> Result<CreateFileResponse> {
    let resolved = resolve_allowed_path(Path::new(path), false)?;
    if resolved.exists() {
        bail!(
            "A file or directory already exists at {}",
            resolved.display()
        );
    }
    fs::create_dir_all(&resolved)
        .with_context(|| format!("Failed to create directory {}", resolved.display()))?;
    Ok(CreateFileResponse {
        absolute_path: resolved.display().to_string(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameRequest {
    pub from_path: String,
    pub to_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameResponse {
    pub from_path: String,
    pub to_path: String,
}

/// Rename / move a file or directory. Both paths are resolved against
/// the workspace allow-list. Refuses to overwrite an existing target.
pub fn rename_path(from_path: &str, to_path: &str) -> Result<RenameResponse> {
    let from = resolve_allowed_path(Path::new(from_path), true)?;
    let to = resolve_allowed_path(Path::new(to_path), false)?;

    if to.exists() {
        bail!("Cannot rename: {} already exists", to.display());
    }
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create parent directory {}", parent.display()))?;
    }

    fs::rename(&from, &to)
        .with_context(|| format!("Failed to rename {} → {}", from.display(), to.display()))?;

    Ok(RenameResponse {
        from_path: from.display().to_string(),
        to_path: to.display().to_string(),
    })
}

/// Delete a file or directory tree. Defensive checks:
///   - Refuses to delete the workspace root itself.
///   - Refuses to follow symlinks pointing outside the workspace
///     (resolve_allowed_path rejects those at parse time).
pub fn delete_path(path: &str) -> Result<()> {
    let resolved = resolve_allowed_path(Path::new(path), true)?;
    let metadata = fs::symlink_metadata(&resolved)
        .with_context(|| format!("Failed to stat {}", resolved.display()))?;

    if metadata.is_dir() {
        fs::remove_dir_all(&resolved)
            .with_context(|| format!("Failed to delete directory {}", resolved.display()))?;
    } else {
        fs::remove_file(&resolved)
            .with_context(|| format!("Failed to delete file {}", resolved.display()))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use tempfile::tempdir;

    fn with_workspace_dir<F: FnOnce(&Path)>(f: F) {
        let dir = tempdir().unwrap();
        let prev = env::var("WINTHORPE_DATA_DIR").ok();
        env::set_var("WINTHORPE_DATA_DIR", dir.path());
        f(dir.path());
        match prev {
            Some(v) => env::set_var("WINTHORPE_DATA_DIR", v),
            None => env::remove_var("WINTHORPE_DATA_DIR"),
        }
    }

    #[test]
    fn create_file_makes_empty_file() {
        with_workspace_dir(|dir| {
            let path = dir.join("workspaces").join("repo").join("ws").join("a.txt");
            create_file(path.to_str().unwrap()).unwrap();
            assert!(path.exists());
            assert_eq!(fs::read(&path).unwrap(), b"");
        });
    }

    #[test]
    fn create_file_creates_intermediate_dirs() {
        with_workspace_dir(|dir| {
            let path = dir
                .join("workspaces")
                .join("repo")
                .join("ws")
                .join("nested")
                .join("deeply")
                .join("a.txt");
            create_file(path.to_str().unwrap()).unwrap();
            assert!(path.exists());
        });
    }

    #[test]
    fn create_file_refuses_existing() {
        with_workspace_dir(|dir| {
            let path = dir.join("workspaces").join("repo").join("ws").join("a.txt");
            create_file(path.to_str().unwrap()).unwrap();
            assert!(create_file(path.to_str().unwrap()).is_err());
        });
    }

    #[test]
    fn create_directory_works() {
        with_workspace_dir(|dir| {
            let path = dir
                .join("workspaces")
                .join("repo")
                .join("ws")
                .join("subdir");
            create_directory(path.to_str().unwrap()).unwrap();
            assert!(path.is_dir());
        });
    }

    #[test]
    fn rename_path_moves_file() {
        with_workspace_dir(|dir| {
            let from = dir.join("workspaces").join("repo").join("ws").join("a.txt");
            let to = dir.join("workspaces").join("repo").join("ws").join("b.txt");
            create_file(from.to_str().unwrap()).unwrap();
            rename_path(from.to_str().unwrap(), to.to_str().unwrap()).unwrap();
            assert!(!from.exists());
            assert!(to.exists());
        });
    }

    #[test]
    fn rename_path_refuses_overwrite() {
        with_workspace_dir(|dir| {
            let a = dir.join("workspaces").join("repo").join("ws").join("a.txt");
            let b = dir.join("workspaces").join("repo").join("ws").join("b.txt");
            create_file(a.to_str().unwrap()).unwrap();
            create_file(b.to_str().unwrap()).unwrap();
            assert!(rename_path(a.to_str().unwrap(), b.to_str().unwrap()).is_err());
        });
    }

    #[test]
    fn delete_path_removes_file() {
        with_workspace_dir(|dir| {
            let path = dir.join("workspaces").join("repo").join("ws").join("a.txt");
            create_file(path.to_str().unwrap()).unwrap();
            delete_path(path.to_str().unwrap()).unwrap();
            assert!(!path.exists());
        });
    }

    #[test]
    fn delete_path_removes_directory_recursively() {
        with_workspace_dir(|dir| {
            let nested = dir
                .join("workspaces")
                .join("repo")
                .join("ws")
                .join("subdir")
                .join("inner.txt");
            create_file(nested.to_str().unwrap()).unwrap();
            let parent = nested.parent().unwrap().to_path_buf();
            delete_path(parent.to_str().unwrap()).unwrap();
            assert!(!parent.exists());
        });
    }
}
