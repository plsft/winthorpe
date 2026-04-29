//! PTY-backed interactive script + terminal execution.
//!
//! Cross-platform via `portable-pty` — ConPTY on Windows, POSIX PTYs on Unix.
//! See `pty.rs` for the unified impl.
//!
//! The legacy `unix.rs` module is kept around (compiled cfg(unix), behind a
//! `legacy` feature flag) for one release as a fallback in case portable-pty
//! turns out to have a regression we can't fix in the wrapper. Phase 9
//! removes it once we're confident.

mod pty;

#[cfg(all(unix, feature = "legacy-pty"))]
mod unix;

pub use pty::{run_script, run_terminal_session, ScriptContext, ScriptEvent, ScriptProcessManager};
