//! Filesystem watcher for the workspace file tree + open editor tabs.
//!
//! Distinct from `git/watcher.rs` which monitors `.git/HEAD` + refs for
//! branch-change autofetch. This watcher monitors the **whole workspace
//! directory** (recursively) and emits a single `workspace-files-changed`
//! Tauri event whenever anything inside it changes.
//!
//! Frontend uses the event to:
//!   - Invalidate the workspaceTree React Query → tree refreshes.
//!   - For each open editor tab whose path appears in the event payload:
//!     reload the file content if the tab isn't dirty, otherwise show a
//!     "file changed externally" toast with a Reload action.
//!
//! Implementation: notify-debouncer-full coalesces FS events over a
//! 250ms window so a "save 50 files" git checkout doesn't spam the UI.
//! A single watcher is active at a time (whichever workspace is
//! currently selected); switching workspaces stops the old watcher and
//! starts a new one.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context, Result};
use notify::RecursiveMode;
use notify_debouncer_full::{new_debouncer, Debouncer, RecommendedCache};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

pub const WORKSPACE_FILES_CHANGED_EVENT: &str = "workspace-files-changed";

/// Debounce window. Long enough to coalesce a `git checkout` of a busy
/// branch (typically <100ms of staggered file writes); short enough that
/// the UI feels reactive after a single editor save.
const DEBOUNCE_INTERVAL: Duration = Duration::from_millis(250);

/// Path components we ignore in event payloads. We DON'T pass them to
/// the watcher's recurse-mode filter (notify doesn't expose one cheaply);
/// we just drop matching events at debounce-flush time.
const IGNORED_DIR_NAMES: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".turbo",
    ".cache",
    ".bundle-cache",
];

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFilesChangedPayload {
    pub workspace_id: String,
    /// Absolute paths (forward-slash normalized) of changed entries.
    /// Bounded — see PAYLOAD_PATH_LIMIT — so a runaway directory rewrite
    /// can't blow up the IPC message size.
    pub paths: Vec<String>,
}

const PAYLOAD_PATH_LIMIT: usize = 200;

struct ActiveWatcher {
    workspace_id: String,
    root: PathBuf,
    _debouncer: Debouncer<notify::RecommendedWatcher, RecommendedCache>,
}

#[derive(Default)]
pub struct WorkspaceFilesWatcherManager {
    active: Mutex<HashMap<String, ActiveWatcher>>,
}

impl WorkspaceFilesWatcherManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start watching `root` and emit events tagged with `workspace_id`.
    /// If a watcher is already running for the same workspace_id with the
    /// same root, this is a no-op. If the root changed, the old watcher
    /// is stopped before the new one starts.
    pub fn start<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        workspace_id: String,
        root: PathBuf,
    ) -> Result<()> {
        let mut map = self
            .active
            .lock()
            .map_err(|_| anyhow::anyhow!("watcher map poisoned"))?;

        // Idempotent: same root + same id → keep existing watcher.
        if let Some(existing) = map.get(&workspace_id) {
            if existing.root == root {
                return Ok(());
            }
        }

        // Drop any prior watcher for this workspace. The Drop on
        // ActiveWatcher releases the inotify/ReadDirectoryChangesW handle.
        map.remove(&workspace_id);

        let app_handle = app.clone();
        let workspace_id_for_event = workspace_id.clone();

        let mut debouncer = new_debouncer(
            DEBOUNCE_INTERVAL,
            None,
            move |result: notify_debouncer_full::DebounceEventResult| {
                let events = match result {
                    Ok(events) => events,
                    Err(errors) => {
                        for error in errors {
                            tracing::debug!(error = %error, "files watcher error");
                        }
                        return;
                    }
                };

                let mut paths: Vec<String> = Vec::new();
                for event in events {
                    for path in &event.paths {
                        if path_in_ignored_dir(path) {
                            continue;
                        }
                        let normalized = path.display().to_string().replace('\\', "/");
                        paths.push(normalized);
                        if paths.len() >= PAYLOAD_PATH_LIMIT {
                            break;
                        }
                    }
                    if paths.len() >= PAYLOAD_PATH_LIMIT {
                        break;
                    }
                }

                paths.sort();
                paths.dedup();
                if paths.is_empty() {
                    return;
                }

                let payload = WorkspaceFilesChangedPayload {
                    workspace_id: workspace_id_for_event.clone(),
                    paths,
                };
                if let Err(error) = app_handle.emit(WORKSPACE_FILES_CHANGED_EVENT, payload) {
                    tracing::warn!(
                        error = %error,
                        workspace_id = %workspace_id_for_event,
                        "Failed to emit workspace-files-changed event"
                    );
                }
            },
        )
        .with_context(|| format!("Failed to create FS debouncer for {}", root.display()))?;

        debouncer
            .watch(&root, RecursiveMode::Recursive)
            .with_context(|| format!("Failed to watch {}", root.display()))?;

        map.insert(
            workspace_id.clone(),
            ActiveWatcher {
                workspace_id,
                root,
                _debouncer: debouncer,
            },
        );

        Ok(())
    }

    /// Stop watching for `workspace_id`. No-op if not currently watching.
    pub fn stop(&self, workspace_id: &str) {
        if let Ok(mut map) = self.active.lock() {
            if let Some(watcher) = map.remove(workspace_id) {
                tracing::debug!(
                    workspace_id = %watcher.workspace_id,
                    root = %watcher.root.display(),
                    "Stopped workspace files watcher"
                );
            }
        }
    }
}

/// Returns true when ANY component of `path` matches an ignored dir name.
fn path_in_ignored_dir(path: &std::path::Path) -> bool {
    path.components().any(|c| match c {
        std::path::Component::Normal(name) => name
            .to_str()
            .is_some_and(|s| IGNORED_DIR_NAMES.contains(&s)),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn path_in_ignored_dir_catches_node_modules() {
        assert!(path_in_ignored_dir(Path::new(
            "/repo/node_modules/foo/index.js"
        )));
    }

    #[test]
    fn path_in_ignored_dir_catches_dot_git() {
        assert!(path_in_ignored_dir(Path::new("/repo/.git/HEAD")));
    }

    #[test]
    fn path_in_ignored_dir_passes_normal_files() {
        assert!(!path_in_ignored_dir(Path::new("/repo/src/main.rs")));
    }

    #[test]
    fn path_in_ignored_dir_handles_windows_separators() {
        // Path::components handles forward slashes on every platform; \\
        // separators only resolve as separators on Windows. Test the FS-
        // independent forward-slash form.
        assert!(path_in_ignored_dir(Path::new(
            "C:/repo/target/debug/deps/foo.rmeta"
        )));
    }
}
