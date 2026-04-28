//! Shared helpers for the unified pipeline test infrastructure.
//!
//! Three test targets share these modules:
//! - `pipeline_scenarios.rs` — handcrafted edge-case scenarios (normalized snapshots)
//! - `pipeline_fixtures.rs` — real DB-captured sessions (raw snapshots)
//! - `pipeline_streams.rs` — raw stream-event jsonl replay (synthesized snapshots)
//!
//! Submodule layout:
//! - `normalize`     — normalized snapshot format (`NormPart`, `normalize_*`).
//! - `builders`      — `HistoricalRecord` builders + `run_normalized`.
//! - `stream_replay` — live-pipeline replay fingerprint helpers.
//! - `fixtures`      — on-disk JSON fixture loader.
//!
//! Each test target sees a different subset of these helpers — `dead_code`
//! is permitted globally so unused-from-target's-perspective items don't
//! emit warnings.

// Each test target sees a different subset of these helpers. `dead_code`
// + `unused_imports` keep the targets that don't exercise every helper
// from emitting warnings — the suppression lives on the common module
// itself rather than on every test target.
#![allow(dead_code, unused_imports)]

// Re-exported so test files can `use common::*` and reach the production
// pipeline types without listing each one.
pub use winthorpe_lib::pipeline::types::{HistoricalRecord, ThreadMessageLike};
pub use winthorpe_lib::pipeline::MessagePipeline;

mod builders;
mod fixtures;
mod normalize;
mod stream_replay;

pub use builders::*;
pub use fixtures::*;
pub use normalize::*;
pub use stream_replay::*;
