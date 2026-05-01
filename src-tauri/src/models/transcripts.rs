//! Disk-based session transcript scanner — direct port of worktale's
//! `worktale-plugin/hooks/session-track.mjs` (Claude) and
//! `worktale-codex-plugin/hooks/session-track.mjs` (Codex).
//!
//! Why this approach (rather than parsing in-memory streaming events):
//!   - Both Claude Code and the Codex CLI write authoritative JSONL
//!     transcripts to known locations on disk.
//!   - The token usage fields in those transcripts are **the same fields
//!     the vendors bill from**, so cost computation matches the invoice.
//!   - No coupling to the sidecar's event shape — works for any session
//!     spawned via the bundled CLIs, including ones outside Winthorpe.
//!   - Worktale runs this exact logic in production today; we're not
//!     guessing at field names, we're reusing a verified parser.
//!
//! Architecture:
//!   - `scan_and_record_all()` runs periodically (background task).
//!   - Discovers Claude transcripts under `~/.claude/projects/.../*.jsonl`
//!     and Codex transcripts under `~/.codex/sessions/YYYY/MM/DD/*.jsonl`.
//!   - For each "stale" file (not modified in the last STALE_MIN minutes —
//!     heuristic for "session ended"), parses, computes cost, inserts an
//!     `ai_sessions` row, and marks the file as processed in a state file
//!     under `<data_dir>/transcripts-processed.json`.
//!   - State file is bounded at 500 entries (LRU by recordedAt).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::models::ai_sessions::{self, AiSessionInsert};
use crate::models::pricing;

/// Session is considered "ended" after this many minutes since the last
/// transcript write. Mirrors worktale's STALE_MIN.
const STALE_MIN: u64 = 5;
/// How many days back to scan for unprocessed transcripts. Mirrors
/// worktale's SCAN_DAYS.
const SCAN_DAYS: u64 = 7;
/// Sessions with fewer total tokens than this are skipped — usually
/// abort/no-op invocations. Mirrors worktale's MIN_TOKENS.
const MIN_TOKENS: i64 = 100;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ProcessedState {
    /// Map of file path → processing record. Bounded at 500 entries.
    processed: HashMap<String, ProcessedRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProcessedRecord {
    /// Unix millis when this row was recorded.
    recorded_at: u64,
    status: String,
    cost: Option<f64>,
}

/// Public entry point — scan both providers and record any stale,
/// unprocessed sessions. Best-effort throughout: errors are logged, the
/// scan keeps going for the next file. Safe to call from a background
/// task on any cadence (currently every 60 s).
pub fn scan_and_record_all() -> Result<usize> {
    let mut state = load_state();
    let mut recorded = 0usize;

    recorded += scan_provider(&mut state, claude_transcripts_root(), ProviderKind::Claude);
    recorded += scan_provider(&mut state, codex_transcripts_root(), ProviderKind::Codex);

    trim_state(&mut state, 500);
    save_state(&state);
    Ok(recorded)
}

#[derive(Debug, Clone, Copy)]
enum ProviderKind {
    Claude,
    Codex,
}

fn scan_provider(state: &mut ProcessedState, root: Option<PathBuf>, kind: ProviderKind) -> usize {
    let Some(root) = root else { return 0 };
    if !root.exists() {
        return 0;
    }

    let now_ms = unix_millis_now();
    let stale_cutoff = now_ms.saturating_sub(STALE_MIN * 60 * 1000);
    let scan_cutoff = now_ms.saturating_sub(SCAN_DAYS * 24 * 60 * 60 * 1000);

    let mut files = list_recent_jsonl_files(&root, scan_cutoff);
    files.sort_by_key(|f| f.modified_ms);

    let mut recorded = 0usize;
    for entry in files {
        let key = entry.path.to_string_lossy().to_string();
        if state.processed.contains_key(&key) {
            continue;
        }
        if entry.modified_ms > stale_cutoff {
            // Session is still active — wait for the next scan tick.
            continue;
        }

        match parse_and_record(&entry.path, kind) {
            Ok(Some(cost)) => {
                state.processed.insert(
                    key,
                    ProcessedRecord {
                        recorded_at: now_ms,
                        status: "ok".into(),
                        cost: Some(cost),
                    },
                );
                recorded += 1;
            }
            Ok(None) => {
                // Below threshold or empty — mark as processed so we
                // don't keep re-reading it on every scan tick.
                state.processed.insert(
                    key,
                    ProcessedRecord {
                        recorded_at: now_ms,
                        status: "too-small".into(),
                        cost: None,
                    },
                );
            }
            Err(error) => {
                tracing::warn!(
                    path = %entry.path.display(),
                    error = %error,
                    "transcript parse failed"
                );
                state.processed.insert(
                    key,
                    ProcessedRecord {
                        recorded_at: now_ms,
                        status: "error".into(),
                        cost: None,
                    },
                );
            }
        }
    }
    recorded
}

fn parse_and_record(path: &Path, kind: ProviderKind) -> Result<Option<f64>> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("read transcript {}", path.display()))?;

    let aggregate = match kind {
        ProviderKind::Claude => parse_claude_transcript(&raw),
        ProviderKind::Codex => parse_codex_transcript(&raw),
    };

    let Some(agg) = aggregate else {
        return Ok(None);
    };

    let total_tokens =
        agg.input_tokens + agg.output_tokens + agg.cache_read_tokens + agg.cache_write_tokens;
    if total_tokens < MIN_TOKENS {
        return Ok(None);
    }

    let cost = pricing::cost_for_turn(
        agg.model.as_deref().unwrap_or(""),
        agg.input_tokens,
        agg.output_tokens,
        agg.cache_read_tokens,
        agg.cache_write_tokens,
    );

    let duration_secs = match (agg.first_ts_ms, agg.last_ts_ms) {
        (Some(first), Some(last)) if last >= first => ((last - first) / 1000).max(1) as i64,
        _ => 0,
    };

    let (workspace_id, repo_id, is_pr_create) =
        attribute_session(agg.cwd.as_deref(), agg.session_id.as_deref());

    let insert = AiSessionInsert {
        workspace_id,
        session_id: agg.session_id.clone(),
        repo_id,
        provider: Some(match kind {
            ProviderKind::Claude => "claude".into(),
            ProviderKind::Codex => "codex".into(),
        }),
        model: agg.model.clone(),
        tool: Some(match kind {
            ProviderKind::Claude => "claude-code".into(),
            ProviderKind::Codex => "codex".into(),
        }),
        cost_usd: Some(cost),
        input_tokens: Some(agg.input_tokens),
        output_tokens: Some(agg.output_tokens),
        cache_read_tokens: Some(agg.cache_read_tokens),
        cache_write_tokens: Some(agg.cache_write_tokens),
        tools_used: if agg.tools.is_empty() {
            None
        } else {
            Some(agg.tools.into_iter().collect())
        },
        mcp_servers: if agg.mcp_servers.is_empty() {
            None
        } else {
            Some(agg.mcp_servers.into_iter().collect())
        },
        duration_secs: Some(duration_secs),
        commits: None,
        is_pr_create: Some(is_pr_create),
        note: None,
    };

    // Use the transcript's last-message timestamp so rows show the actual
    // session-end time instead of "all inserted at the same minute" when
    // a first scan picks up weeks of historical files.
    ai_sessions::insert_with_timestamp(insert, agg.last_ts_ms)
        .with_context(|| format!("insert ai_sessions row for {}", path.display()))?;
    tracing::info!(
        provider = ?kind,
        path = %path.display(),
        cost_usd = cost,
        input = agg.input_tokens,
        output = agg.output_tokens,
        cache_read = agg.cache_read_tokens,
        cache_write = agg.cache_write_tokens,
        "Recorded ai_sessions row from transcript"
    );
    Ok(Some(cost))
}

#[derive(Debug, Default)]
struct TranscriptAggregate {
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    model: Option<String>,
    cwd: Option<String>,
    session_id: Option<String>,
    first_ts_ms: Option<u64>,
    last_ts_ms: Option<u64>,
    tools: std::collections::BTreeSet<String>,
    mcp_servers: std::collections::BTreeSet<String>,
}

/// Parse a Claude Code transcript file (JSONL). Direct port of
/// `worktale-plugin/hooks/session-track.mjs::parseInto`.
fn parse_claude_transcript(raw: &str) -> Option<TranscriptAggregate> {
    let mut agg = TranscriptAggregate::default();

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };

        if let Some(ts_ms) = parse_iso_timestamp(value.get("timestamp")) {
            agg.first_ts_ms = Some(agg.first_ts_ms.map_or(ts_ms, |f| f.min(ts_ms)));
            agg.last_ts_ms = Some(agg.last_ts_ms.map_or(ts_ms, |l| l.max(ts_ms)));
        }
        if agg.cwd.is_none() {
            if let Some(cwd) = value.get("cwd").and_then(Value::as_str) {
                if !cwd.is_empty() {
                    agg.cwd = Some(cwd.to_string());
                }
            }
        }

        // Worktale convention: `message` is the inner SDK message object,
        // or the entry itself if there's no nesting.
        let message = value.get("message").unwrap_or(&value);
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }

        if let Some(model) = message.get("model").and_then(Value::as_str) {
            // First non-empty model wins (matches worktale's `primaryModel`).
            if agg.model.is_none() {
                agg.model = Some(model.to_string());
            }
        }

        if let Some(usage) = message.get("usage").and_then(Value::as_object) {
            agg.input_tokens += usage
                .get("input_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            agg.output_tokens += usage
                .get("output_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            agg.cache_read_tokens += usage
                .get("cache_read_input_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            agg.cache_write_tokens += usage
                .get("cache_creation_input_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(0);
        }

        if let Some(content) = message.get("content").and_then(Value::as_array) {
            for block in content {
                if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                    if let Some(name) = block.get("name").and_then(Value::as_str) {
                        agg.tools.insert(name.to_string());
                        if let Some(rest) = name.strip_prefix("mcp__") {
                            if let Some((server, _)) = rest.split_once("__") {
                                agg.mcp_servers.insert(server.to_string());
                            } else {
                                agg.mcp_servers.insert(rest.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    if agg.input_tokens == 0
        && agg.output_tokens == 0
        && agg.cache_read_tokens == 0
        && agg.cache_write_tokens == 0
    {
        return None;
    }
    Some(agg)
}

/// Parse a Codex CLI session JSONL file. Direct port of
/// `worktale-codex-plugin/hooks/session-track.mjs::parseSessionFile`.
fn parse_codex_transcript(raw: &str) -> Option<TranscriptAggregate> {
    let mut agg = TranscriptAggregate::default();
    let mut previous_totals: Option<(i64, i64, i64)> = None; // (input, cached, output)

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };

        if let Some(ts_ms) = parse_iso_timestamp(value.get("timestamp")) {
            agg.first_ts_ms = Some(agg.first_ts_ms.map_or(ts_ms, |f| f.min(ts_ms)));
            agg.last_ts_ms = Some(agg.last_ts_ms.map_or(ts_ms, |l| l.max(ts_ms)));
        }

        let payload = value.get("payload").unwrap_or(&value);
        let event_type = value
            .get("type")
            .or_else(|| payload.get("type"))
            .and_then(Value::as_str)
            .unwrap_or("");

        match event_type {
            "session_meta" => {
                if let Some(cwd) = pick_field_str(payload, &["cwd", "workspace", "working_dir"]) {
                    agg.cwd = Some(cwd);
                }
                if let Some(sid) = pick_field_str(payload, &["id", "session_id", "sessionId"]) {
                    agg.session_id = Some(sid);
                }
            }
            "turn_context" => {
                let model = pick_field_str(payload, &["model"]).or_else(|| {
                    payload.get("info").and_then(|info| {
                        info.get("model").and_then(Value::as_str).map(String::from)
                    })
                });
                if let Some(m) = model {
                    agg.model = Some(m);
                }
            }
            "token_count" => {
                let info = payload.get("info").unwrap_or(payload);
                if agg.model.is_none() {
                    if let Some(m) = pick_field_str(info, &["model"]) {
                        agg.model = Some(m);
                    }
                }

                let total = info
                    .get("total_token_usage")
                    .or_else(|| info.get("totalTokenUsage"));
                let last = info
                    .get("last_token_usage")
                    .or_else(|| info.get("lastTokenUsage"));

                if let Some(total) = total {
                    let t_in = pick_field_i64(total, &["input_tokens", "inputTokens"]).unwrap_or(0);
                    let t_cache = pick_field_i64(
                        total,
                        &[
                            "cached_input_tokens",
                            "cachedInputTokens",
                            "cache_read_input_tokens",
                            "cacheReadInputTokens",
                        ],
                    )
                    .unwrap_or(0);
                    let t_out =
                        pick_field_i64(total, &["output_tokens", "outputTokens"]).unwrap_or(0);

                    if let Some((p_in, p_cache, p_out)) = previous_totals {
                        agg.input_tokens += (t_in - p_in).max(0);
                        agg.cache_read_tokens += (t_cache - p_cache).max(0);
                        agg.output_tokens += (t_out - p_out).max(0);
                    } else {
                        agg.input_tokens += t_in;
                        agg.cache_read_tokens += t_cache;
                        agg.output_tokens += t_out;
                    }
                    previous_totals = Some((t_in, t_cache, t_out));
                } else if let Some(last) = last {
                    let d_in = pick_field_i64(last, &["input_tokens", "inputTokens"]).unwrap_or(0);
                    let d_cache = pick_field_i64(
                        last,
                        &[
                            "cached_input_tokens",
                            "cachedInputTokens",
                            "cache_read_input_tokens",
                            "cacheReadInputTokens",
                        ],
                    )
                    .unwrap_or(0);
                    let d_out =
                        pick_field_i64(last, &["output_tokens", "outputTokens"]).unwrap_or(0);
                    agg.input_tokens += d_in;
                    agg.cache_read_tokens += d_cache;
                    agg.output_tokens += d_out;
                    previous_totals =
                        Some((agg.input_tokens, agg.cache_read_tokens, agg.output_tokens));
                }
            }
            _ => {}
        }
    }

    if agg.input_tokens == 0 && agg.output_tokens == 0 && agg.cache_read_tokens == 0 {
        return None;
    }
    Some(agg)
}

fn pick_field_str(obj: &Value, names: &[&str]) -> Option<String> {
    for n in names {
        if let Some(v) = obj.get(*n).and_then(Value::as_str) {
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn pick_field_i64(obj: &Value, names: &[&str]) -> Option<i64> {
    for n in names {
        if let Some(v) = obj.get(*n).and_then(Value::as_i64) {
            return Some(v);
        }
    }
    None
}

fn parse_iso_timestamp(v: Option<&Value>) -> Option<u64> {
    let s = v.and_then(Value::as_str)?;
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.timestamp_millis() as u64)
}

#[derive(Debug)]
struct TranscriptFile {
    path: PathBuf,
    modified_ms: u64,
}

fn list_recent_jsonl_files(root: &Path, modified_after: u64) -> Vec<TranscriptFile> {
    let mut out = Vec::new();
    walk_jsonl(root, &mut out, modified_after, 0);
    out
}

fn walk_jsonl(dir: &Path, out: &mut Vec<TranscriptFile>, modified_after: u64, depth: u32) {
    if depth > 6 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            walk_jsonl(&path, out, modified_after, depth + 1);
        } else if path.extension().is_some_and(|ext| ext == "jsonl") {
            let modified_ms = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            if modified_ms >= modified_after {
                out.push(TranscriptFile { path, modified_ms });
            }
        }
    }
}

fn claude_transcripts_root() -> Option<PathBuf> {
    let home = home_dir()?;
    Some(home.join(".claude").join("projects"))
}

fn codex_transcripts_root() -> Option<PathBuf> {
    if let Ok(env) = std::env::var("CODEX_HOME") {
        return Some(PathBuf::from(env).join("sessions"));
    }
    let home = home_dir()?;
    Some(home.join(".codex").join("sessions"))
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

fn unix_millis_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn state_file_path() -> Option<PathBuf> {
    crate::data_dir::data_dir()
        .ok()
        .map(|d| d.join("transcripts-processed.json"))
}

fn load_state() -> ProcessedState {
    let Some(path) = state_file_path() else {
        return ProcessedState::default();
    };
    let Ok(raw) = fs::read_to_string(&path) else {
        return ProcessedState::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

/// Delete the processed-state file. After this runs, the next scan
/// reconsiders every transcript on disk within `SCAN_DAYS`. Pairs with
/// `ai_sessions::delete_all` to fully reset the ledger from the GUI.
pub fn reset_processed_state() -> Result<()> {
    let Some(path) = state_file_path() else {
        return Ok(());
    };
    if !path.exists() {
        return Ok(());
    }
    fs::remove_file(&path).with_context(|| format!("Failed to delete {}", path.display()))?;
    Ok(())
}

fn save_state(state: &ProcessedState) {
    let Some(path) = state_file_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let Ok(serialized) = serde_json::to_string_pretty(state) else {
        return;
    };
    let _ = fs::write(&path, serialized);
}

fn trim_state(state: &mut ProcessedState, max_entries: usize) {
    if state.processed.len() <= max_entries {
        return;
    }
    let mut entries: Vec<(String, ProcessedRecord)> = state.processed.drain().collect();
    entries.sort_by_key(|(_, rec)| std::cmp::Reverse(rec.recorded_at));
    entries.truncate(max_entries);
    state.processed = entries.into_iter().collect();
}

/// Resolve `(workspace_id, repo_id, is_pr_create)` from cwd and/or
/// session_id. Best-effort: missing matches just leave fields null.
///
/// - workspace lookup tries to match the cwd against known workspace
///   `worktree_path` columns.
/// - is_pr_create lookups via the session_id → action_kind path.
fn attribute_session(
    cwd: Option<&str>,
    session_id: Option<&str>,
) -> (Option<String>, Option<String>, bool) {
    let Ok(conn) = crate::models::db::read_conn() else {
        return (None, None, false);
    };

    // First, try to attribute via session_id → workspace_id directly.
    if let Some(sid) = session_id {
        if let Ok((wid, rid, is_pr)) = conn.query_row(
            "SELECT s.workspace_id, w.repository_id, s.action_kind
             FROM sessions s
             LEFT JOIN workspaces w ON w.id = s.workspace_id
             WHERE s.id = ?1",
            rusqlite::params![sid],
            |row| {
                let wid: Option<String> = row.get(0)?;
                let rid: Option<String> = row.get(1)?;
                let action_kind: Option<String> = row.get(2)?;
                Ok((wid, rid, action_kind.as_deref() == Some("create-pr")))
            },
        ) {
            return (wid, rid, is_pr);
        }
    }

    // Fallback: match cwd to a workspace by directory_name + repo path.
    // Cheap LIKE query — workspaces tend to be < 100 rows total.
    if let Some(cwd) = cwd {
        let cwd_norm = cwd.replace('/', "\\").to_ascii_lowercase();
        if let Ok((wid, rid)) = conn.query_row(
            "SELECT w.id, w.repository_id
             FROM workspaces w
             WHERE LOWER(REPLACE(w.directory_name, '/', '\\')) != ''
               AND ?1 LIKE '%' || LOWER(w.directory_name)
             LIMIT 1",
            rusqlite::params![cwd_norm],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        ) {
            return (Some(wid), rid, false);
        }
    }

    (None, None, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_claude_transcript_aggregates_usage() {
        let raw = r#"{"timestamp":"2025-10-20T10:00:00Z","cwd":"/work/repo","message":{"role":"assistant","model":"claude-sonnet-4-5","usage":{"input_tokens":1000,"output_tokens":500,"cache_read_input_tokens":2000,"cache_creation_input_tokens":300},"content":[{"type":"tool_use","name":"Bash"}]}}
{"timestamp":"2025-10-20T10:01:00Z","message":{"role":"user","content":"hi"}}
{"timestamp":"2025-10-20T10:02:00Z","message":{"role":"assistant","model":"claude-sonnet-4-5","usage":{"input_tokens":50,"output_tokens":100,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"content":[{"type":"tool_use","name":"mcp__github__create_pr"}]}}"#;

        let agg = parse_claude_transcript(raw).unwrap();
        assert_eq!(agg.input_tokens, 1050);
        assert_eq!(agg.output_tokens, 600);
        assert_eq!(agg.cache_read_tokens, 2000);
        assert_eq!(agg.cache_write_tokens, 300);
        assert_eq!(agg.model.as_deref(), Some("claude-sonnet-4-5"));
        assert_eq!(agg.cwd.as_deref(), Some("/work/repo"));
        assert!(agg.tools.contains("Bash"));
        assert!(agg.tools.contains("mcp__github__create_pr"));
        assert!(agg.mcp_servers.contains("github"));
    }

    #[test]
    fn parse_codex_transcript_uses_total_deltas() {
        // Codex emits cumulative totals — successive entries should be
        // diffed so we don't double-count.
        let raw = r#"{"timestamp":"2025-10-20T10:00:00Z","type":"session_meta","payload":{"cwd":"/work/repo","id":"sess-1"}}
{"timestamp":"2025-10-20T10:00:01Z","type":"turn_context","payload":{"model":"gpt-5"}}
{"timestamp":"2025-10-20T10:00:10Z","type":"token_count","payload":{"info":{"total_token_usage":{"input_tokens":500,"output_tokens":100,"cached_input_tokens":0}}}}
{"timestamp":"2025-10-20T10:00:30Z","type":"token_count","payload":{"info":{"total_token_usage":{"input_tokens":1500,"output_tokens":400,"cached_input_tokens":200}}}}"#;

        let agg = parse_codex_transcript(raw).unwrap();
        assert_eq!(agg.input_tokens, 1500);
        assert_eq!(agg.output_tokens, 400);
        assert_eq!(agg.cache_read_tokens, 200);
        assert_eq!(agg.model.as_deref(), Some("gpt-5"));
        assert_eq!(agg.cwd.as_deref(), Some("/work/repo"));
        assert_eq!(agg.session_id.as_deref(), Some("sess-1"));
    }
}
