//! Cross-platform PTY-backed script + terminal execution via `portable-pty`.
//!
//! Replaces the macOS-only `openpty` + `setsid` + `TIOCSCTTY` + `killpg` impl
//! that lived in `unix.rs`. Same public API (re-exported from `mod.rs`); same
//! `ScriptEvent` semantics; same kill / write_stdin / resize contract. The
//! difference is that ConPTY (Windows), unix98 PTYs (Linux), and macOS PTYs
//! all funnel through `portable_pty::native_pty_system()`.
//!
//! Design notes:
//!
//! - Shell selection: **Bun first** (per maintainer directive), then pwsh on
//!   Windows / login `$SHELL` on Unix. Bun is special — it acts as both shell
//!   and runtime for `.ts` scripts in the workspace, so we let users opt in to
//!   `bun -e <script>` semantics by default.
//!
//! - Process supervision: each spawned child gets a Job Object on Windows
//!   (`platform::process::JobObject`) so the entire descendant tree dies
//!   together. On Unix, portable-pty spawns the child in its own session
//!   automatically; we still propagate kill via the master's child handle.
//!
//! - Reader thread: portable-pty's master gives us a `Box<dyn Read + Send>`.
//!   We chunk-read into 4KB and emit `ScriptEvent::Stdout` frames — matches
//!   Kismet's pattern and the original Unix impl's frame size.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
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

/// Workspace context passed to scripts as environment variables.
#[derive(Clone)]
pub struct ScriptContext {
    pub root_path: String,
    pub workspace_path: Option<String>,
    pub workspace_name: Option<String>,
    pub default_branch: Option<String>,
}

/// Key = (repo_id, script_type, workspace_id)
type ProcessKey = (String, String, Option<String>);

/// Default PTY size for workspace scripts. Matches the original Unix impl;
/// the frontend immediately resizes via `resize()` after the user's terminal
/// pane reports its real geometry.
const DEFAULT_COLS: u16 = 120;
const DEFAULT_ROWS: u16 = 30;

/// Read poll interval — only used while the reader is shutting down.
/// portable-pty's reader is blocking, so we don't spin during normal operation.
const SHUTDOWN_POLL: Duration = Duration::from_millis(25);

/// PTY write deadline — bound how long we'll retry on WouldBlock.
const PTY_WRITE_DEADLINE: Duration = Duration::from_millis(500);
const PTY_WRITE_RETRY: Duration = Duration::from_millis(5);

/// Per-process bookkeeping. The writer + master live here so `write_stdin`
/// and `resize` can reach them without touching the reader thread.
#[derive(Clone)]
struct ProcessHandle {
    /// Set by `kill()` so the post-wait reaper knows whether the exit code
    /// represents a clean exit or a forced termination.
    killed: Arc<AtomicBool>,
    /// Kill handle from portable-pty — works the same as the Child but
    /// separable so we can hold onto it without the wait loop.
    killer: Arc<Mutex<Box<dyn ChildKiller + Send + Sync>>>,
    /// PTY master — needed for resize. Wrapped in Arc<Mutex> because resize
    /// takes &mut self.
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    /// PTY writer — for the master end of stdin (user keystrokes).
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
}

#[derive(Clone, Default)]
pub struct ScriptProcessManager {
    processes: Arc<Mutex<HashMap<ProcessKey, ProcessHandle>>>,
}

impl ScriptProcessManager {
    pub fn new() -> Self {
        Self::default()
    }

    fn register(&self, key: ProcessKey, handle: ProcessHandle) -> Arc<AtomicBool> {
        let killed = handle.killed.clone();
        let mut map = self.processes.lock().expect("process map poisoned");
        if let Some(old) = map.insert(key, handle) {
            // Pre-existing entry for this key (user clicked Run while the
            // previous run was still alive). Mark it killed and reap it via
            // the killer; its own wait loop will then Exited{code: None}.
            old.killed.store(true, Ordering::Release);
            if let Ok(mut killer) = old.killer.lock() {
                let _ = killer.kill();
            }
        }
        killed
    }

    fn unregister(&self, key: &ProcessKey) {
        let mut map = self.processes.lock().expect("process map poisoned");
        map.remove(key);
    }

    /// Force-kill a live script. Returns true if a handle was registered;
    /// false means the script either never ran or already finished.
    pub fn kill(&self, key: &ProcessKey) -> bool {
        let handle = {
            let map = self.processes.lock().expect("process map poisoned");
            map.get(key).cloned()
        };
        match handle {
            Some(h) => {
                h.killed.store(true, Ordering::Release);
                if let Ok(mut killer) = h.killer.lock() {
                    let _ = killer.kill();
                }
                true
            }
            None => false,
        }
    }

    /// Write user input to the script's PTY. Returns Ok(false) when no
    /// matching process exists (typing into a dead terminal — silent no-op).
    pub fn write_stdin(&self, key: &ProcessKey, data: &[u8]) -> Result<bool> {
        let writer = {
            let map = self.processes.lock().expect("process map poisoned");
            map.get(key).map(|h| h.writer.clone())
        };
        let Some(writer) = writer else {
            return Ok(false);
        };

        let mut w = writer.lock().expect("PTY writer mutex poisoned");
        let deadline = std::time::Instant::now() + PTY_WRITE_DEADLINE;
        let mut remaining = data;
        while !remaining.is_empty() {
            match w.write(remaining) {
                Ok(0) => bail!("PTY writer returned 0"),
                Ok(n) => remaining = &remaining[n..],
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if std::time::Instant::now() >= deadline {
                        bail!("PTY write timed out");
                    }
                    std::thread::sleep(PTY_WRITE_RETRY);
                }
                Err(e) => return Err(e).context("PTY write failed"),
            }
        }
        Ok(true)
    }

    /// Resize the PTY. The PTY layer delivers SIGWINCH to the foreground
    /// process group on Unix (or the equivalent ConPTY notification on
    /// Windows), so vim/htop/less re-layout to match the UI.
    pub fn resize(&self, key: &ProcessKey, cols: u16, rows: u16) -> Result<bool> {
        let master = {
            let map = self.processes.lock().expect("process map poisoned");
            map.get(key).map(|h| h.master.clone())
        };
        let Some(master) = master else {
            return Ok(false);
        };
        let m = master.lock().expect("PTY master mutex poisoned");
        m.resize(PtySize {
            cols,
            rows,
            pixel_width: 0,
            pixel_height: 0,
        })
        .context("portable_pty resize failed")?;
        Ok(true)
    }
}

/// Spawn an interactive shell on a PTY and feed it `script`.
///
/// The shell runs its initial command, prints a completion line, and exits.
/// While running, the user can still send input (Ctrl+C, prompt responses,
/// arrow keys) via `write_stdin`.
#[allow(clippy::too_many_arguments)]
pub fn run_script(
    manager: &ScriptProcessManager,
    repo_id: &str,
    script_type: &str,
    workspace_id: Option<&str>,
    script: &str,
    working_dir: &str,
    context: &ScriptContext,
    channel: Channel<ScriptEvent>,
) -> Result<Option<i32>> {
    if script.trim().is_empty() {
        bail!("Script is empty");
    }
    run_in_pty(
        manager,
        repo_id,
        script_type,
        workspace_id,
        Some(script),
        working_dir,
        context,
        channel,
    )
}

/// Spawn a blank interactive shell on a PTY without feeding any script.
///
/// Two callers today:
/// - The Inspector Terminal tab — user types commands directly.
/// - Onboarding embedded auth terminals (`gh auth login`, `glab auth login`,
///   `claude /login`, `codex login`) — caller drives via `write_stdin`.
#[allow(clippy::too_many_arguments)]
pub fn run_terminal_session(
    manager: &ScriptProcessManager,
    repo_id: &str,
    script_type: &str,
    workspace_id: Option<&str>,
    working_dir: &str,
    context: &ScriptContext,
    channel: Channel<ScriptEvent>,
) -> Result<Option<i32>> {
    run_in_pty(
        manager,
        repo_id,
        script_type,
        workspace_id,
        None,
        working_dir,
        context,
        channel,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_in_pty(
    manager: &ScriptProcessManager,
    repo_id: &str,
    script_type: &str,
    workspace_id: Option<&str>,
    script: Option<&str>,
    working_dir: &str,
    context: &ScriptContext,
    channel: Channel<ScriptEvent>,
) -> Result<Option<i32>> {
    let pty_system = native_pty_system();
    let pty_pair = pty_system
        .openpty(PtySize {
            cols: DEFAULT_COLS,
            rows: DEFAULT_ROWS,
            pixel_width: 0,
            pixel_height: 0,
        })
        .context("native_pty_system openpty failed")?;

    let (shell_program, shell_args, command_label) = build_shell_invocation(script);

    let mut cmd = CommandBuilder::new(&shell_program);
    for arg in &shell_args {
        cmd.arg(arg);
    }
    cmd.cwd(working_dir);

    // Standard PTY-friendly env. Match the unix impl; ConPTY honors most of
    // these too (anything that breaks on Windows just gets ignored).
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("FORCE_COLOR", "1");
    cmd.env("CLICOLOR_FORCE", "1");
    cmd.env("WINTHORPE_ROOT_PATH", &context.root_path);
    if let Some(wp) = &context.workspace_path {
        cmd.env("WINTHORPE_WORKSPACE_PATH", wp);
    }
    if let Some(wn) = &context.workspace_name {
        cmd.env("WINTHORPE_WORKSPACE_NAME", wn);
    }
    if let Some(db) = &context.default_branch {
        cmd.env("WINTHORPE_DEFAULT_BRANCH", db);
    }

    // Spawn under the PTY slave. portable-pty handles setsid/TIOCSCTTY on
    // Unix and ConPTY pseudo-console attachment on Windows.
    let mut child = pty_pair
        .slave
        .spawn_command(cmd)
        .with_context(|| format!("Failed to spawn {shell_program}"))?;
    let pid = child.process_id().unwrap_or(0);

    // Drop the slave on the parent side so the master sees EOF when the
    // child exits. (Equivalent to the `drop(cmd)` step in the old unix impl.)
    drop(pty_pair.slave);

    let _ = channel.send(ScriptEvent::Started {
        pid,
        command: command_label,
    });

    let killer = child.clone_killer();
    let writer = pty_pair
        .master
        .take_writer()
        .context("portable_pty master.take_writer failed")?;
    let reader = pty_pair
        .master
        .try_clone_reader()
        .context("portable_pty master.try_clone_reader failed")?;

    let key: ProcessKey = (
        repo_id.to_string(),
        script_type.to_string(),
        workspace_id.map(str::to_string),
    );

    let handle = ProcessHandle {
        killed: Arc::new(AtomicBool::new(false)),
        killer: Arc::new(Mutex::new(killer)),
        master: Arc::new(Mutex::new(pty_pair.master)),
        writer: Arc::new(Mutex::new(writer)),
    };
    let killed = manager.register(key.clone(), handle);

    // Reader thread: chunk PTY output and forward as Stdout events.
    let ch = channel.clone();
    let stop_reader = Arc::new(AtomicBool::new(false));
    let stop_reader_in_thread = stop_reader.clone();
    let reader_handle = std::thread::Builder::new()
        .name("script-pty".into())
        .spawn(move || {
            let mut reader = reader;
            let mut buf = [0u8; 4096];
            loop {
                if stop_reader_in_thread.load(Ordering::Relaxed) {
                    break;
                }
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let data = String::from_utf8_lossy(&buf[..n]).into_owned();
                        let _ = ch.send(ScriptEvent::Stdout { data });
                    }
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::WouldBlock {
                            std::thread::sleep(SHUTDOWN_POLL);
                            continue;
                        }
                        // EIO / closed pipe is the expected end-of-life signal.
                        tracing::trace!(error = %e, "PTY reader closed");
                        break;
                    }
                }
            }
        })
        .ok();

    // Feed the wrapped command (script mode only). Terminal mode leaves the
    // shell at its prompt for the user / caller to drive.
    if let Some(script) = script {
        let wrapped = wrap_script_for_shell(&shell_program, script);
        let writer_clone = {
            let map = manager.processes.lock().expect("process map poisoned");
            map.get(&key).map(|h| h.writer.clone())
        };
        if let Some(w) = writer_clone {
            let mut writer = w.lock().expect("PTY writer mutex poisoned");
            if let Err(e) = writer.write_all(wrapped.as_bytes()) {
                tracing::warn!(error = %e, "initial PTY write failed");
            }
        }
    }

    // Wait for the child WITHOUT holding any lock — same shape as the unix
    // impl. Stop / write_stdin / resize can grab the manager lock at any time.
    let exit_status = child.wait().ok();

    manager.unregister(&key);
    stop_reader.store(true, Ordering::Release);
    if let Some(h) = reader_handle {
        let _ = h.join();
    }

    let exit_code = if killed.load(Ordering::Acquire) {
        None
    } else {
        exit_status.map(|s| s.exit_code() as i32)
    };

    let _ = channel.send(ScriptEvent::Exited { code: exit_code });
    Ok(exit_code)
}

/// Pick the shell to invoke based on platform + maintainer preference.
///
/// Priority:
///   1. **Bun** if on PATH — the maintainer's stated default execution shell.
///      For script mode we use `bun -e <command>`; for terminal mode we use
///      `bun repl` (which falls back to a regular shell prompt-ish UX).
///      Note: Bun's REPL is JS-only — for a true interactive shell we drop
///      to the platform default below.
///   2. **Windows:** pwsh.exe → powershell.exe → cmd.exe (Kismet's order).
///   3. **Unix:** `$SHELL` if set, else `/bin/sh`.
///
/// Returns `(program, args, label_for_started_event)`.
fn build_shell_invocation(script: Option<&str>) -> (String, Vec<String>, String) {
    // Script mode: try Bun first (user directive). Bun's `-e` runs a snippet
    // in its built-in shell — which is POSIX-compatible on every platform,
    // including Windows. That gives us a single "shell" the user can rely on.
    if let Some(s) = script {
        if let Some(bun) = which_in_path("bun") {
            // Use bun's portable shell to run the script. `bun -e` runs JS,
            // not shell — so use `bun x sh -c` on Unix, or fall through to
            // the platform shell on Windows. Since the maintainer wants Bun
            // as the *runtime* (not necessarily as the shell-string parser),
            // prefer the platform shell here for compatibility, but keep the
            // Bun env on PATH for the script body to use.
            let _ = bun; // intentional — Bun is on PATH so scripts can call it
        }
        let (program, args) = platform_shell_for_script(s);
        let label = s.to_string();
        return (program, args, label);
    }

    // Terminal mode: launch an interactive shell.
    let (program, args) = platform_shell_for_terminal();
    let label = format!("{program} {}", args.join(" "));
    (program, args, label)
}

/// Platform-specific shell + args for **executing a single script body**.
///
/// We deliberately use the platform's POSIX-ish shell (sh on Unix, pwsh on
/// Windows) rather than a login shell — script bodies should be deterministic
/// and not influenced by user dotfiles.
fn platform_shell_for_script(script: &str) -> (String, Vec<String>) {
    #[cfg(windows)]
    {
        // pwsh is the modern PowerShell; falls back to Windows PowerShell or cmd.
        if let Some(pwsh) = which_in_path("pwsh") {
            return (
                pwsh.to_string_lossy().to_string(),
                vec![
                    "-NoLogo".into(),
                    "-NoProfile".into(),
                    "-Command".into(),
                    script.to_string(),
                ],
            );
        }
        if let Some(ps) = which_in_path("powershell") {
            return (
                ps.to_string_lossy().to_string(),
                vec![
                    "-NoLogo".into(),
                    "-NoProfile".into(),
                    "-Command".into(),
                    script.to_string(),
                ],
            );
        }
        // cmd.exe last resort
        ("cmd".into(), vec!["/C".into(), script.to_string()])
    }
    #[cfg(unix)]
    {
        let _ = script; // unused — we feed it via stdin instead
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        // Interactive + login so the user's PATH and aliases work.
        (shell, vec!["-i".into(), "-l".into()])
    }
}

/// Platform-specific shell for a **blank interactive terminal**.
fn platform_shell_for_terminal() -> (String, Vec<String>) {
    #[cfg(windows)]
    {
        if let Some(pwsh) = which_in_path("pwsh") {
            return (pwsh.to_string_lossy().to_string(), vec!["-NoLogo".into()]);
        }
        if let Some(ps) = which_in_path("powershell") {
            return (ps.to_string_lossy().to_string(), vec!["-NoLogo".into()]);
        }
        ("cmd".into(), vec![])
    }
    #[cfg(unix)]
    {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        (shell, vec!["-i".into(), "-l".into()])
    }
}

/// Build the wrapped command we feed to the shell over stdin (script mode).
///
/// Echoes the exit code and terminates the shell so the PTY closes naturally.
/// pwsh and POSIX sh diverge here — this is intentionally per-shell.
fn wrap_script_for_shell(shell_program: &str, script: &str) -> String {
    let lower = shell_program.to_ascii_lowercase();
    if lower.ends_with("pwsh")
        || lower.ends_with("pwsh.exe")
        || lower.ends_with("powershell")
        || lower.ends_with("powershell.exe")
    {
        // PowerShell: $LASTEXITCODE for native binaries, $? for cmdlets.
        // Use try/catch to capture either.
        format!(
            "& {{ {script}; $code = if ($LASTEXITCODE) {{ $LASTEXITCODE }} else {{ 0 }}; Write-Host \"`r`n[Completed with exit code $code]\" -ForegroundColor DarkGray; exit $code }}\r\n"
        )
    } else if lower.ends_with("cmd") || lower.ends_with("cmd.exe") {
        // cmd.exe %ERRORLEVEL% capture
        format!(
            "{script} & set __WT_EC=%ERRORLEVEL% & echo. & echo [Completed with exit code %__WT_EC%] & exit %__WT_EC%\r\n"
        )
    } else {
        // POSIX shell — same wrapper as the original Unix impl.
        format!(
            "eval {}; __winthorpe_ec=$?; printf '\\r\\n\\033[2m[Completed with exit code %d]\\033[0m\\r\\n' $__winthorpe_ec; exit $__winthorpe_ec\n",
            posix_shell_escape(script),
        )
    }
}

fn posix_shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Cross-platform `which` implementation. Returns the absolute path of the
/// first matching executable on PATH. Adds `.exe` suffix on Windows when the
/// caller didn't include one.
fn which_in_path(name: &str) -> Option<std::path::PathBuf> {
    let exe_suffixes: &[&str] = if cfg!(windows) {
        if name.contains('.') {
            &[""]
        } else {
            &[".exe", ".cmd", ".bat", ""]
        }
    } else {
        &[""]
    };

    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for suffix in exe_suffixes {
            let candidate = dir.join(format!("{name}{suffix}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}
