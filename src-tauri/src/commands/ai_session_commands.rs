//! Tauri commands for the per-turn token + cost ledger.
//!
//! Schema mirrors worktale's `ai_sessions` so a future export-to-worktale
//! sync is a straight column copy.

use super::common::{run_blocking, CmdResult};
use crate::models::ai_sessions::{self, AiSession, AiSessionInsert, AiSessionStats};

/// Insert one turn's tokens + cost. Called from the streaming pipeline
/// when a turn finalizes; the streaming pipeline owns parsing the
/// provider's usage events and converting model + token counts into
/// `cost_usd` via the pricing table (see `pricing.rs`, follow-up turn).
#[tauri::command]
pub async fn record_ai_session(record: AiSessionInsert) -> CmdResult<i64> {
    run_blocking(move || ai_sessions::insert(record)).await
}

#[tauri::command]
pub async fn list_ai_sessions_for_workspace(
    workspace_id: String,
    limit: Option<i64>,
) -> CmdResult<Vec<AiSession>> {
    run_blocking(move || ai_sessions::list_for_workspace(&workspace_id, limit.unwrap_or(200))).await
}

#[tauri::command]
pub async fn list_recent_ai_sessions(limit: Option<i64>) -> CmdResult<Vec<AiSession>> {
    run_blocking(move || ai_sessions::list_recent(limit.unwrap_or(500))).await
}

#[tauri::command]
pub async fn get_ai_session_stats() -> CmdResult<AiSessionStats> {
    run_blocking(ai_sessions::stats_overall).await
}

#[tauri::command]
pub async fn get_last_pr_cost_for_workspace(workspace_id: String) -> CmdResult<f64> {
    run_blocking(move || ai_sessions::last_pr_cost_for_workspace(&workspace_id)).await
}

/// Wipe the entire cost ledger AND the transcript-processed state file
/// so the next background scan starts from scratch. Returns how many
/// rows were deleted so the UI can show "Cleared 1,247 sessions."
#[tauri::command]
pub async fn reset_ai_session_ledger() -> CmdResult<i64> {
    run_blocking(|| {
        let deleted = ai_sessions::delete_all()?;
        // Best-effort: even if state-file deletion fails, the row delete
        // already happened. Surface a warning via tracing instead of
        // blocking the user on a non-critical cleanup step.
        if let Err(error) = crate::models::transcripts::reset_processed_state() {
            tracing::warn!(
                error = %error,
                "Reset ledger: deleted DB rows but failed to clear processed-state file. \
                 Next scan may re-record older sessions."
            );
        }
        Ok(deleted)
    })
    .await
}
