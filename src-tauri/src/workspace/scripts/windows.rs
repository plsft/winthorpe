//! Windows stub for the PTY-backed script runner.
//!
//! Phase 2 replaces this module with a `portable-pty` impl that uses ConPTY
//! under the hood. Until then, every entry point returns a "not yet
//! implemented" error and emits an `Error` event on the channel so the UI
//! can show a sensible message instead of hanging.
//!
//! API parity with `unix.rs` is intentional — the type names, fields, and
//! function signatures match exactly so callers compile unchanged.

use std::sync::Arc;

use anyhow::{bail, Result};
use serde::Serialize;
use tauri::ipc::Channel;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ScriptEvent {
    Started { pid: u32, command: String },
    Stdout { data: String },
    Stderr { data: String },
    Exited { code: Option<i32> },
    Error { message: String },
}

/// Shared state placeholder. The Phase 2 impl will own a process registry
/// (key → handle) the same way the Unix impl does today.
#[derive(Clone, Default)]
pub struct ScriptProcessManager {
    _inner: Arc<()>,
}

impl ScriptProcessManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Stub: returns false (no live handle to kill).
    pub fn kill(&self, _key: &(String, String, Option<String>)) -> bool {
        false
    }

    /// Stub: returns Ok(false) — caller treats false as "no live script", which
    /// matches the Unix behavior when the key isn't registered.
    pub fn write_stdin(
        &self,
        _key: &(String, String, Option<String>),
        _data: &[u8],
    ) -> Result<bool> {
        Ok(false)
    }

    /// Stub: returns Ok(false) — same rationale as `write_stdin`.
    pub fn resize(
        &self,
        _key: &(String, String, Option<String>),
        _cols: u16,
        _rows: u16,
    ) -> Result<bool> {
        Ok(false)
    }
}

/// Workspace context passed to scripts as environment variables.
///
/// Field names + types match the Unix impl exactly so Phase 2 can collapse
/// the two definitions into one.
#[derive(Clone)]
pub struct ScriptContext {
    pub root_path: String,
    pub workspace_path: Option<String>,
    pub workspace_name: Option<String>,
    pub default_branch: Option<String>,
}

#[allow(clippy::too_many_arguments)]
pub fn run_script(
    _manager: &ScriptProcessManager,
    _repo_id: &str,
    _script_type: &str,
    _workspace_id: Option<&str>,
    _script: &str,
    _working_dir: &str,
    _context: &ScriptContext,
    channel: Channel<ScriptEvent>,
) -> Result<Option<i32>> {
    let message =
        "Script execution on Windows is not yet implemented (Phase 2).".to_string();
    let _ = channel.send(ScriptEvent::Error {
        message: message.clone(),
    });
    bail!(message)
}

#[allow(clippy::too_many_arguments)]
pub fn run_terminal_session(
    _manager: &ScriptProcessManager,
    _repo_id: &str,
    _script_type: &str,
    _workspace_id: Option<&str>,
    _working_dir: &str,
    _context: &ScriptContext,
    channel: Channel<ScriptEvent>,
) -> Result<Option<i32>> {
    let message =
        "Interactive terminal sessions on Windows are not yet implemented (Phase 2).".to_string();
    let _ = channel.send(ScriptEvent::Error {
        message: message.clone(),
    });
    bail!(message)
}
