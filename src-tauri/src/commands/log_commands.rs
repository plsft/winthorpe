//! Tauri commands backing the GUI's Activity Log panel.
//!
//! Frontend usage:
//!   - On open: call `get_log_events(0, 500)` for the initial backfill.
//!   - Then poll every ~750 ms with `get_log_events(lastSeq, 500)` for the
//!     tail. Polling beats event subscription here because it lets the
//!     frontend pace itself when the user scrolls back in history.
//!   - Optional: `clear_log_events()` to start fresh.

use serde::Serialize;

use super::common::CmdResult;
use crate::logging_buffer::{self, LogEvent};

/// Page of log events plus a cursor the frontend hands back on the next call.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogPage {
    pub events: Vec<LogEvent>,
    /// Largest `seq` in this page (or the input `since` if the page is
    /// empty). Hand this back as `since` on the next poll.
    pub next_since: u64,
    /// True when the buffer holds more events newer than what we returned —
    /// the frontend should poll again immediately rather than waiting for
    /// the regular interval. Caps the latency on bursty workloads.
    pub has_more: bool,
}

/// Fetch up to `limit` events newer than `since`. Pass `since = 0` for the
/// first call to backfill from the start of the buffer.
#[tauri::command]
pub async fn get_log_events(since: u64, limit: u32) -> CmdResult<LogPage> {
    let limit = limit.clamp(1, 2000) as usize;
    let events = logging_buffer::snapshot_since(since, limit + 1);

    // Detect overflow without paying for an extra `clone` on the full
    // 501st event — we just need to know if it's there.
    let has_more = events.len() > limit;
    let trimmed: Vec<LogEvent> = events.into_iter().take(limit).collect();
    let next_since = trimmed.last().map(|e| e.seq).unwrap_or(since);

    Ok(LogPage {
        events: trimmed,
        next_since,
        has_more,
    })
}

/// Drop every buffered event. The seq counter is preserved so an open
/// frontend can keep polling without resync confusion.
#[tauri::command]
pub async fn clear_log_events() -> CmdResult<()> {
    logging_buffer::clear();
    Ok(())
}
