//! Workspace directory tree listing for the file-explorer sidebar.
//!
//! Distinct from `editor.rs::list_editor_files` which is git-aware and
//! caps results at 24 files (sized for the inspector's "Recent files"
//! widget). The file explorer wants the FULL tree of files + directories
//! so the user can drill into any folder.
//!
//! Walks the workspace recursively, skipping the usual heavyweight dirs
//! (`.git`, `node_modules`, `target`, `dist`, etc.) so the response is
//! small enough to ship in one IPC call. Returns a flat list — the
//! frontend builds the hierarchical tree from the relative paths.
//!
//! No git status, no diff data — just the directory structure. The
//! editor-files endpoint stays the source of truth for git-aware UI.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

use super::support::resolve_allowed_path;

/// Hard upper bound to keep one bad workspace from blowing up the IPC
/// payload. ~10k entries is plenty for everything but enormous monorepos;
/// those should split or use the editor-files inspector list instead.
const MAX_TREE_ENTRIES: usize = 10_000;

/// Directory names skipped during the walk. Matches the conventional
/// "build artifacts + VCS metadata + IDE noise" exclusion set.
const IGNORED_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    ".next",
    ".nuxt",
    ".turbo",
    ".cache",
    ".bundle-cache",
    ".vite",
    ".svelte-kit",
    ".angular",
    ".gradle",
    ".idea",
    ".vscode-test",
    ".pytest_cache",
    "__pycache__",
    ".venv",
    "venv",
    ".tox",
    ".mypy_cache",
    ".ruff_cache",
];

/// File names skipped during the walk.
const IGNORED_FILES: &[&str] = &[".DS_Store", "Thumbs.db", "desktop.ini"];

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceTreeEntryKind {
    File,
    Directory,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceTreeEntry {
    /// Path relative to the workspace root, forward-slash normalized.
    /// Empty string would only occur for the root itself, which we skip.
    pub path: String,
    /// Final path component (file or directory name).
    pub name: String,
    /// Absolute on-disk path. Used by the editor open flow to bypass
    /// re-resolving the workspace root on every click.
    pub absolute_path: String,
    /// File or Directory.
    pub kind: WorkspaceTreeEntryKind,
}

pub fn list_workspace_tree(workspace_root_path: &str) -> Result<Vec<WorkspaceTreeEntry>> {
    let workspace_root = resolve_allowed_path(Path::new(workspace_root_path), true)
        .with_context(|| format!("Workspace root not accessible: {workspace_root_path}"))?;

    if !workspace_root.is_dir() {
        anyhow::bail!(
            "Workspace root is not a directory: {}",
            workspace_root.display()
        );
    }

    let mut entries: Vec<WorkspaceTreeEntry> = Vec::new();
    walk_dir(&workspace_root, &workspace_root, &mut entries)?;

    if entries.len() > MAX_TREE_ENTRIES {
        // Keep the first MAX_TREE_ENTRIES (alphabetical order means the
        // truncation is deterministic across calls).
        tracing::warn!(
            workspace = %workspace_root.display(),
            total = entries.len(),
            cap = MAX_TREE_ENTRIES,
            "Workspace tree truncated"
        );
        entries.truncate(MAX_TREE_ENTRIES);
    }

    Ok(entries)
}

fn walk_dir(dir: &Path, workspace_root: &Path, out: &mut Vec<WorkspaceTreeEntry>) -> Result<()> {
    if out.len() >= MAX_TREE_ENTRIES {
        return Ok(());
    }

    let read = match fs::read_dir(dir) {
        Ok(read) => read,
        Err(error) => {
            // Permission errors on subdirs are common (Library/, .Trash/
            // on macOS; protected admin dirs on Windows). Skip silently.
            tracing::debug!(
                dir = %dir.display(),
                error = %error,
                "Skipping unreadable directory"
            );
            return Ok(());
        }
    };

    // Collect children so we can sort: directories first, then files,
    // alphabetical within each group (ignore-case for nicer mixed-case dirs).
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut files: Vec<PathBuf> = Vec::new();

    for entry in read.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        if path.is_dir() {
            if IGNORED_DIRS.contains(&name) {
                continue;
            }
            dirs.push(path);
        } else if path.is_file() {
            if IGNORED_FILES.contains(&name) {
                continue;
            }
            files.push(path);
        }
    }

    dirs.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    files.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));

    // Emit directories with their entries inline (depth-first) so the flat
    // list reads like a pre-order traversal — convenient for building the
    // tree on the client without a sort step.
    for d in dirs {
        if let Some(entry) = build_entry(&d, workspace_root, WorkspaceTreeEntryKind::Directory) {
            out.push(entry);
        }
        walk_dir(&d, workspace_root, out)?;
        if out.len() >= MAX_TREE_ENTRIES {
            return Ok(());
        }
    }

    for f in files {
        if let Some(entry) = build_entry(&f, workspace_root, WorkspaceTreeEntryKind::File) {
            out.push(entry);
        }
        if out.len() >= MAX_TREE_ENTRIES {
            return Ok(());
        }
    }

    Ok(())
}

fn build_entry(
    path: &Path,
    workspace_root: &Path,
    kind: WorkspaceTreeEntryKind,
) -> Option<WorkspaceTreeEntry> {
    let relative = path.strip_prefix(workspace_root).ok()?;
    let name = path.file_name()?.to_string_lossy().to_string();
    Some(WorkspaceTreeEntry {
        path: relative.to_string_lossy().replace('\\', "/"),
        name,
        absolute_path: path.display().to_string(),
        kind,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn entry_paths(entries: &[WorkspaceTreeEntry]) -> Vec<&str> {
        entries.iter().map(|e| e.path.as_str()).collect()
    }

    #[test]
    fn lists_root_files_and_dirs() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.rs"), "").unwrap();
        fs::write(dir.path().join("b.txt"), "").unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src").join("main.rs"), "").unwrap();

        let tree = list_workspace_tree(dir.path().to_str().unwrap()).unwrap();
        let paths = entry_paths(&tree);
        // Directories first, then files. Pre-order: src, src/main.rs, a.rs, b.txt
        assert_eq!(paths, vec!["src", "src/main.rs", "a.rs", "b.txt"]);
    }

    #[test]
    fn skips_ignored_directories() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("node_modules").join("foo")).unwrap();
        fs::write(dir.path().join("node_modules").join("foo").join("x.js"), "").unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join(".git").join("HEAD"), "ref").unwrap();
        fs::write(dir.path().join("README.md"), "# x").unwrap();

        let tree = list_workspace_tree(dir.path().to_str().unwrap()).unwrap();
        let paths = entry_paths(&tree);
        assert_eq!(paths, vec!["README.md"]);
    }

    #[test]
    fn classifies_kind_correctly() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("subdir")).unwrap();
        fs::write(dir.path().join("file.txt"), "").unwrap();

        let tree = list_workspace_tree(dir.path().to_str().unwrap()).unwrap();
        let kinds: Vec<_> = tree.iter().map(|e| (e.path.as_str(), e.kind)).collect();
        assert_eq!(
            kinds,
            vec![
                ("subdir", WorkspaceTreeEntryKind::Directory),
                ("file.txt", WorkspaceTreeEntryKind::File),
            ]
        );
    }

    #[test]
    fn skips_os_metadata_files() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(".DS_Store"), "").unwrap();
        fs::write(dir.path().join("Thumbs.db"), "").unwrap();
        fs::write(dir.path().join("real.txt"), "").unwrap();

        let tree = list_workspace_tree(dir.path().to_str().unwrap()).unwrap();
        assert_eq!(entry_paths(&tree), vec!["real.txt"]);
    }

    #[test]
    fn relative_paths_use_forward_slashes() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("a").join("b")).unwrap();
        fs::write(dir.path().join("a").join("b").join("c.txt"), "").unwrap();

        let tree = list_workspace_tree(dir.path().to_str().unwrap()).unwrap();
        let leaf = tree.iter().find(|e| e.name == "c.txt").unwrap();
        assert_eq!(leaf.path, "a/b/c.txt");
    }

    #[test]
    fn empty_workspace_returns_empty_list() {
        let dir = tempdir().unwrap();
        let tree = list_workspace_tree(dir.path().to_str().unwrap()).unwrap();
        assert!(tree.is_empty());
    }

    #[test]
    fn case_insensitive_alphabetical_order() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("Zebra.txt"), "").unwrap();
        fs::write(dir.path().join("apple.txt"), "").unwrap();
        fs::write(dir.path().join("banana.txt"), "").unwrap();

        let tree = list_workspace_tree(dir.path().to_str().unwrap()).unwrap();
        // Lowercased sort key: apple, banana, Zebra
        assert_eq!(
            entry_paths(&tree),
            vec!["apple.txt", "banana.txt", "Zebra.txt"]
        );
    }
}
