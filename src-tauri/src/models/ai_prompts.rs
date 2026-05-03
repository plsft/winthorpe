//! Per-prompt history stream — every user prompt typed into a CLI
//! agent (Claude Code, Codex, GitHub Copilot CLI), regardless of whether
//! that session was launched from inside Winthorpe.
//!
//! Sources:
//!   - `~/.claude/history.jsonl`           (display, pastedContents, ts ms, project)
//!   - `~/.codex/history.jsonl`            (session_id, ts seconds, text)
//!   - `~/.copilot/command-history-state.json` (commandHistory[] strings)
//!   - the inline user-message records inside per-session transcripts
//!
//! Insertion is idempotent via the unique index
//! `(provider, source_file, sequence)` so the scanner can be re-run on
//! every tick without duplicating rows.

use anyhow::{Context, Result};
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::db;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiPrompt {
    pub id: i64,
    pub provider: String,
    pub source: String,
    pub source_file: Option<String>,
    pub sequence: i64,
    pub session_id: Option<String>,
    pub workspace_id: Option<String>,
    pub repo_id: Option<String>,
    pub project_path: Option<String>,
    pub git_branch: Option<String>,
    pub prompt: String,
    pub prompt_length: i64,
    pub has_paste: bool,
    pub timestamp_ms: Option<i64>,
    pub date: Option<String>,
    pub recorded_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct AiPromptInsert {
    pub provider: String,
    pub source: String,
    pub source_file: Option<String>,
    pub sequence: i64,
    pub session_id: Option<String>,
    pub workspace_id: Option<String>,
    pub repo_id: Option<String>,
    pub project_path: Option<String>,
    pub git_branch: Option<String>,
    pub prompt: String,
    pub has_paste: bool,
    pub timestamp_ms: Option<i64>,
}

/// Insert one prompt row. Silent no-op if a row with the same
/// `(provider, source_file, sequence)` already exists — that's the
/// idempotency guarantee. Returns the rowid of the new row, or 0 when
/// the conflict suppressed the insert.
pub fn insert(record: AiPromptInsert) -> Result<i64> {
    let conn = db::write_conn()?;
    insert_with_conn(&conn, record)
}

/// Bulk insert with a caller-owned connection, useful when the scanner
/// wants to write thousands of rows in a single transaction.
pub fn insert_with_conn(conn: &rusqlite::Connection, record: AiPromptInsert) -> Result<i64> {
    let date = record.timestamp_ms.and_then(|ms| {
        chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms).map(|dt| {
            dt.with_timezone(&chrono::Local)
                .format("%Y-%m-%d")
                .to_string()
        })
    });
    let length = record.prompt.chars().count() as i64;
    let affected = conn
        .execute(
            "INSERT OR IGNORE INTO ai_prompts (
                provider, source, source_file, sequence,
                session_id, workspace_id, repo_id, project_path, git_branch,
                prompt, prompt_length, has_paste,
                timestamp_ms, date
             ) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
            params![
                record.provider,
                record.source,
                record.source_file,
                record.sequence,
                record.session_id,
                record.workspace_id,
                record.repo_id,
                record.project_path,
                record.git_branch,
                record.prompt,
                length,
                i64::from(record.has_paste),
                record.timestamp_ms,
                date,
            ],
        )
        .context("Failed to insert ai_prompt row")?;
    if affected == 0 {
        return Ok(0);
    }
    Ok(conn.last_insert_rowid())
}

pub fn list_recent(limit: i64) -> Result<Vec<AiPrompt>> {
    let conn = db::read_conn()?;
    let mut stmt = conn.prepare(
        "SELECT id, provider, source, source_file, sequence,
                session_id, workspace_id, repo_id, project_path, git_branch,
                prompt, prompt_length, has_paste, timestamp_ms, date, recorded_at
         FROM ai_prompts
         ORDER BY COALESCE(timestamp_ms, 0) DESC, id DESC
         LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![limit], row_to_prompt)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn list_for_workspace(workspace_id: &str, limit: i64) -> Result<Vec<AiPrompt>> {
    let conn = db::read_conn()?;
    let mut stmt = conn.prepare(
        "SELECT id, provider, source, source_file, sequence,
                session_id, workspace_id, repo_id, project_path, git_branch,
                prompt, prompt_length, has_paste, timestamp_ms, date, recorded_at
         FROM ai_prompts
         WHERE workspace_id = ?1
         ORDER BY COALESCE(timestamp_ms, 0) DESC, id DESC
         LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![workspace_id, limit], row_to_prompt)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Truncate every row. Pairs with `transcripts::reset_processed_state`
/// to fully reset the prompt history from the GUI.
pub fn delete_all() -> Result<i64> {
    let conn = db::write_conn()?;
    let affected = conn
        .execute("DELETE FROM ai_prompts", [])
        .context("Failed to truncate ai_prompts")?;
    Ok(affected as i64)
}

fn row_to_prompt(row: &rusqlite::Row<'_>) -> rusqlite::Result<AiPrompt> {
    Ok(AiPrompt {
        id: row.get(0)?,
        provider: row.get(1)?,
        source: row.get(2)?,
        source_file: row.get(3)?,
        sequence: row.get(4)?,
        session_id: row.get(5)?,
        workspace_id: row.get(6)?,
        repo_id: row.get(7)?,
        project_path: row.get(8)?,
        git_branch: row.get(9)?,
        prompt: row.get(10)?,
        prompt_length: row.get(11)?,
        has_paste: row.get::<_, i64>(12)? != 0,
        timestamp_ms: row.get(13)?,
        date: row.get(14)?,
        recorded_at: row.get(15)?,
    })
}
