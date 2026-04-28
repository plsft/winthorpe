//! PTY-backed interactive script + terminal execution.
//!
//! Phase 1 (current): Unix uses the original POSIX implementation
//! (`openpty` + `setsid` + `TIOCSCTTY` + `killpg`). Windows uses a stub that
//! returns "not yet implemented" errors so the rest of the codebase compiles.
//!
//! Phase 2 will replace **both** branches with a unified `portable-pty` impl
//! that targets ConPTY on Windows and POSIX PTYs on Unix. The public API
//! re-exported from this module is the contract Phase 2 must preserve.

#[cfg(unix)]
mod unix;

#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use unix::{
    run_script, run_terminal_session, ScriptContext, ScriptEvent, ScriptProcessManager,
};

#[cfg(windows)]
pub use windows::{
    run_script, run_terminal_session, ScriptContext, ScriptEvent, ScriptProcessManager,
};
