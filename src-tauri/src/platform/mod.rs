//! Cross-platform abstractions used by the rest of the codebase.
//!
//! Pattern: every submodule exposes a *single* public API that does the right
//! thing on each OS. Avoid scattering `cfg` blocks across the codebase — push
//! them down here so callers stay platform-agnostic.
//!
//! Module owners by phase:
//!   - `process`   — Phase 2: Job Objects (Windows) + process groups (Unix)
//!   - `pty`       — Phase 2 owns the portable-pty wrapper; for now lives in
//!                    `workspace/scripts/`; this slot is reserved for the
//!                    eventual move once we deduplicate.
//!   - `credentials` — Phase 4: SQLite + DPAPI store
//!   - `editors`   — Phase 5: registry App Paths walk on Windows, mdfind
//!                    fallback on macOS
//!   - `shell_env` — Phase 1 already lives at the crate root; will move here
//!                    when the platform module gets richer

pub mod credentials;
pub mod process;
