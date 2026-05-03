//! Disk-based session transcript scanner. Reads everything the bundled
//! AI CLIs write to `~/.claude`, `~/.codex`, and `~/.copilot`, and turns
//! it into rows in `ai_sessions` (per-session cost ledger) and
//! `ai_prompts` (per-prompt history).
//!
//! Why this approach (rather than parsing in-memory streaming events):
//!   - All three CLIs persist authoritative JSONL transcripts on disk.
//!   - The token usage fields in those transcripts are **the same fields
//!     the vendors bill from**, so cost computation matches the invoice.
//!   - No coupling to the sidecar's event shape — works for any session
//!     spawned via the bundled CLIs, including ones outside Winthorpe.
//!
//! Architecture:
//!   - `scan_and_record_all()` runs every 60 s on a background thread.
//!   - Discovers transcripts under each provider's well-known path.
//!   - For each "stale" file (not modified in the last STALE_MIN minutes —
//!     heuristic for "session ended"), parses, computes cost, inserts an
//!     `ai_sessions` row, and marks the file as processed in a state file
//!     under `<data_dir>/transcripts-processed.json`.
//!   - History files (`history.jsonl`, `command-history-state.json`) are
//!     ingested into `ai_prompts` on every tick using their byte-length
//!     as a watermark, so we only parse newly appended bytes.
//!   - Per-session transcript user messages are also mirrored to
//!     `ai_prompts` so prompts are durable even if a `history.jsonl` is
//!     rotated out from under us.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::models::ai_prompts::{self, AiPromptInsert};
use crate::models::ai_sessions::{self, AiSessionInsert};
use crate::models::pricing;

/// Session is considered "ended" after this many minutes since the last
/// transcript write. Mirrors worktale's STALE_MIN.
const STALE_MIN: u64 = 5;
/// How many days back to scan for unprocessed transcripts. Mirrors
/// worktale's SCAN_DAYS.
const SCAN_DAYS: u64 = 7;
/// Sessions with fewer total tokens than this are skipped — usually
/// abort/no-op invocations. Mirrors worktale's MIN_TOKENS. Codex
/// transcripts that pre-date the `token_count` event have no usage data;
/// those go through a different gate (turn-count based) so they're not
/// silently dropped.
const MIN_TOKENS: i64 = 100;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ProcessedState {
    /// Map of file path → processing record. Bounded at 500 entries.
    processed: HashMap<String, ProcessedRecord>,
    /// History-file ingest watermarks, keyed by absolute path. Stores
    /// the byte length of the file at the last successful ingest so the
    /// next pass only parses newly appended bytes.
    #[serde(default)]
    history_watermarks: HashMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProcessedRecord {
    /// Unix millis when this row was recorded.
    recorded_at: u64,
    status: String,
    cost: Option<f64>,
}

/// Public entry point — scan every provider and record any stale,
/// unprocessed sessions plus any newly appended history. Best-effort
/// throughout: errors are logged, the scan keeps going for the next file.
pub fn scan_and_record_all() -> Result<usize> {
    let mut state = load_state();
    let mut recorded = 0usize;

    recorded += scan_provider(&mut state, claude_transcripts_root(), ProviderKind::Claude);
    recorded += scan_provider(&mut state, codex_transcripts_root(), ProviderKind::Codex);
    recorded += scan_provider(
        &mut state,
        copilot_transcripts_root(),
        ProviderKind::Copilot,
    );

    // History streams. These are append-only files (mostly), so we track
    // a byte-length watermark and only parse the new tail on each pass.
    ingest_claude_history(&mut state);
    ingest_codex_history(&mut state);
    ingest_copilot_history(&mut state);

    trim_state(&mut state, 5_000);
    save_state(&state);
    Ok(recorded)
}

#[derive(Debug, Clone, Copy)]
enum ProviderKind {
    Claude,
    Codex,
    Copilot,
}

impl ProviderKind {
    fn provider_id(self) -> &'static str {
        match self {
            ProviderKind::Claude => "claude",
            ProviderKind::Codex => "codex",
            ProviderKind::Copilot => "copilot",
        }
    }

    fn tool_id(self) -> &'static str {
        match self {
            ProviderKind::Claude => "claude-code",
            ProviderKind::Codex => "codex",
            ProviderKind::Copilot => "copilot-cli",
        }
    }
}

fn scan_provider(state: &mut ProcessedState, root: Option<PathBuf>, kind: ProviderKind) -> usize {
    let Some(root) = root else { return 0 };
    if !root.exists() {
        return 0;
    }

    let now_ms = unix_millis_now();
    let stale_cutoff = now_ms.saturating_sub(STALE_MIN * 60 * 1000);
    let scan_cutoff = now_ms.saturating_sub(SCAN_DAYS * 24 * 60 * 60 * 1000);

    let target_name = match kind {
        ProviderKind::Copilot => Some("events.jsonl"),
        _ => None,
    };
    let mut files = list_recent_jsonl_files(&root, scan_cutoff, target_name);
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
        ProviderKind::Copilot => parse_copilot_transcript(&raw),
    };

    let Some(agg) = aggregate else {
        return Ok(None);
    };

    let total_tokens =
        agg.input_tokens + agg.output_tokens + agg.cache_read_tokens + agg.cache_write_tokens();

    // Gate per provider: Claude/Codex sessions need real token usage
    // before we record them. Copilot CLI doesn't expose tokens at all,
    // so use turn count as the proxy "did anything happen?" gate.
    let is_meaningful = match kind {
        ProviderKind::Claude => total_tokens >= MIN_TOKENS,
        ProviderKind::Codex => {
            // Newer Codex sessions emit `token_count` events — gate on tokens.
            // Older ones don't, so fall back to turn count + reasoning.
            total_tokens >= MIN_TOKENS
                || (agg.turn_count >= 1 && (agg.tool_call_count > 0 || agg.reasoning_count > 0))
        }
        ProviderKind::Copilot => agg.turn_count >= 1 && agg.tool_call_count >= 1,
    };
    if !is_meaningful {
        return Ok(None);
    }

    let cost = pricing::cost_for_turn_v2(
        agg.model.as_deref().unwrap_or(""),
        agg.input_tokens,
        agg.output_tokens,
        agg.cache_read_tokens,
        agg.cache_5m_tokens,
        agg.cache_1h_tokens,
        agg.web_search_requests,
        agg.web_fetch_requests,
    );

    let duration_secs = match (agg.first_ts_ms, agg.last_ts_ms) {
        (Some(first), Some(last)) if last >= first => ((last - first) / 1000).max(1) as i64,
        _ => 0,
    };

    let (workspace_id, repo_id, is_pr_create) = attribute_session(
        agg.cwd.as_deref(),
        agg.session_id.as_deref(),
        agg.git_branch.as_deref(),
    );

    // Emit per-prompt rows for everything we saw inside this transcript.
    // Each prompt is keyed by its sequence (line number) within the
    // transcript file, so re-scans are idempotent via the unique index.
    if !agg.prompts.is_empty() {
        let source_path = path.to_string_lossy().to_string();
        for prompt in &agg.prompts {
            if prompt.text.trim().is_empty() {
                continue;
            }
            let _ = ai_prompts::insert(AiPromptInsert {
                provider: kind.provider_id().into(),
                source: "transcript".into(),
                source_file: Some(source_path.clone()),
                sequence: prompt.sequence,
                session_id: agg.session_id.clone(),
                workspace_id: workspace_id.clone(),
                repo_id: repo_id.clone(),
                project_path: agg.cwd.clone(),
                git_branch: agg.git_branch.clone(),
                prompt: prompt.text.clone(),
                has_paste: prompt.has_paste,
                timestamp_ms: prompt.timestamp_ms.map(|m| m as i64),
            });
        }
    }

    let extras = build_extras(&agg);

    let insert = AiSessionInsert {
        workspace_id,
        session_id: agg.session_id.clone(),
        repo_id,
        provider: Some(kind.provider_id().into()),
        model: agg.model.clone(),
        tool: Some(kind.tool_id().into()),
        cost_usd: Some(cost),
        input_tokens: Some(agg.input_tokens),
        output_tokens: Some(agg.output_tokens),
        cache_read_tokens: Some(agg.cache_read_tokens),
        cache_write_tokens: Some(agg.cache_write_tokens()),
        tools_used: if agg.tools.is_empty() {
            None
        } else {
            Some(agg.tools.iter().cloned().collect())
        },
        mcp_servers: if agg.mcp_servers.is_empty() {
            None
        } else {
            Some(agg.mcp_servers.iter().cloned().collect())
        },
        duration_secs: Some(duration_secs),
        commits: None,
        is_pr_create: Some(is_pr_create),
        note: None,
        git_branch: agg.git_branch.clone(),
        ai_title: agg.ai_title.clone(),
        client_version: agg.client_version.clone(),
        entrypoint: agg.entrypoint.clone(),
        user_type: agg.user_type.clone(),
        slug: agg.slug.clone(),
        inference_geo: agg.inference_geo.clone(),
        cache_5m_tokens: Some(agg.cache_5m_tokens),
        cache_1h_tokens: Some(agg.cache_1h_tokens),
        web_search_requests: Some(agg.web_search_requests),
        web_fetch_requests: Some(agg.web_fetch_requests),
        service_tier: agg.service_tier.clone(),
        speed: agg.speed.clone(),
        turn_count: Some(agg.turn_count),
        tool_call_count: Some(agg.tool_call_count),
        sidechain_turn_count: Some(agg.sidechain_turn_count),
        subagent_count: Some(agg.subagent_count),
        iteration_count: Some(agg.iteration_count),
        error_count: Some(agg.error_count),
        interrupted_tool_count: Some(agg.interrupted_tool_count),
        permission_mode: agg.permission_mode.clone(),
        stop_reasons: if agg.stop_reasons.is_empty() {
            None
        } else {
            Some(agg.stop_reasons.iter().cloned().collect())
        },
        hook_executions: if agg.hook_executions.is_empty() {
            None
        } else {
            Some(serde_json::to_value(&agg.hook_executions).unwrap_or(Value::Null))
        },
        skills_used: if agg.skills.is_empty() {
            None
        } else {
            Some(agg.skills.iter().cloned().collect())
        },
        plan_mode_used: Some(agg.plan_mode_used),
        approval_policy: agg.approval_policy.clone(),
        sandbox_mode: agg.sandbox_mode.clone(),
        network_access: agg.network_access.clone(),
        instructions_present: Some(agg.instructions_present),
        reasoning_count: Some(agg.reasoning_count),
        escalated_permission_count: Some(agg.escalated_permission_count),
        extras,
    };

    ai_sessions::insert_with_timestamp(insert, agg.last_ts_ms)
        .with_context(|| format!("insert ai_sessions row for {}", path.display()))?;
    tracing::info!(
        provider = ?kind,
        path = %path.display(),
        cost_usd = cost,
        input = agg.input_tokens,
        output = agg.output_tokens,
        cache_read = agg.cache_read_tokens,
        cache_5m = agg.cache_5m_tokens,
        cache_1h = agg.cache_1h_tokens,
        web_search = agg.web_search_requests,
        web_fetch = agg.web_fetch_requests,
        turns = agg.turn_count,
        "Recorded ai_sessions row from transcript"
    );
    Ok(Some(cost))
}

fn build_extras(agg: &TranscriptAggregate) -> Option<Value> {
    let mut map = serde_json::Map::new();
    if !agg.attachments_seen.is_empty() {
        map.insert(
            "attachmentTypes".into(),
            json!(agg.attachments_seen.iter().cloned().collect::<Vec<_>>()),
        );
    }
    if !agg.codex_function_calls.is_empty() {
        map.insert(
            "codexFunctionCalls".into(),
            json!(agg.codex_function_calls.iter().cloned().collect::<Vec<_>>()),
        );
    }
    if agg.codex_total_shell_secs > 0 {
        map.insert(
            "codexTotalShellSecs".into(),
            json!(agg.codex_total_shell_secs),
        );
    }
    if let Some(slug) = &agg.session_slug {
        map.insert("sessionSlug".into(), json!(slug));
    }
    if !agg.permission_modes_seen.is_empty() {
        map.insert(
            "permissionModesSeen".into(),
            json!(agg
                .permission_modes_seen
                .iter()
                .cloned()
                .collect::<Vec<_>>()),
        );
    }
    if !agg.copilot_subagents.is_empty() {
        map.insert(
            "copilotSubagents".into(),
            json!(agg.copilot_subagents.iter().cloned().collect::<Vec<_>>()),
        );
    }
    if let Some(error) = &agg.copilot_error {
        map.insert("copilotError".into(), json!(error));
    }
    if map.is_empty() {
        None
    } else {
        Some(Value::Object(map))
    }
}

#[derive(Debug, Default)]
struct TranscriptAggregate {
    // Token usage.
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_5m_tokens: i64,
    cache_1h_tokens: i64,
    web_search_requests: i64,
    web_fetch_requests: i64,

    // Provenance.
    model: Option<String>,
    cwd: Option<String>,
    session_id: Option<String>,
    git_branch: Option<String>,
    ai_title: Option<String>,
    client_version: Option<String>,
    entrypoint: Option<String>,
    user_type: Option<String>,
    slug: Option<String>,
    session_slug: Option<String>,
    inference_geo: Option<String>,
    service_tier: Option<String>,
    speed: Option<String>,
    permission_mode: Option<String>,

    first_ts_ms: Option<u64>,
    last_ts_ms: Option<u64>,

    // Behavior counters.
    turn_count: i64,
    tool_call_count: i64,
    sidechain_turn_count: i64,
    subagent_count: i64,
    iteration_count: i64,
    error_count: i64,
    interrupted_tool_count: i64,
    reasoning_count: i64,
    escalated_permission_count: i64,
    plan_mode_used: bool,
    instructions_present: bool,

    tools: BTreeSet<String>,
    mcp_servers: BTreeSet<String>,
    skills: BTreeSet<String>,
    stop_reasons: BTreeSet<String>,
    permission_modes_seen: BTreeSet<String>,
    attachments_seen: BTreeSet<String>,
    hook_executions: HashMap<String, HookStat>,

    // Codex-specific.
    approval_policy: Option<String>,
    sandbox_mode: Option<String>,
    network_access: Option<String>,
    codex_function_calls: BTreeSet<String>,
    codex_total_shell_secs: i64,

    // Copilot-specific.
    copilot_subagents: BTreeSet<String>,
    copilot_error: Option<String>,

    // Per-prompt records to mirror into ai_prompts.
    prompts: Vec<PromptRecord>,
}

impl TranscriptAggregate {
    fn cache_write_tokens(&self) -> i64 {
        self.cache_5m_tokens + self.cache_1h_tokens
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct HookStat {
    count: i64,
    total_ms: i64,
    error_count: i64,
}

#[derive(Debug)]
struct PromptRecord {
    sequence: i64,
    text: String,
    has_paste: bool,
    timestamp_ms: Option<u64>,
}

// =============================================================================
// Claude transcript parser
// =============================================================================

/// Parse a Claude Code transcript file (JSONL). Captures everything the
/// envelope and message body expose — see module docs.
fn parse_claude_transcript(raw: &str) -> Option<TranscriptAggregate> {
    let mut agg = TranscriptAggregate::default();
    let mut saw_any_assistant = false;

    for (idx, line) in raw.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };

        // Envelope fields appear on most record types. Capture the first
        // non-empty value per field — the envelope is ~stable across the
        // session.
        capture_first(
            &mut agg.cwd,
            value.get("cwd").and_then(Value::as_str),
            "cwd",
        );
        capture_first(
            &mut agg.git_branch,
            value.get("gitBranch").and_then(Value::as_str),
            "gitBranch",
        );
        capture_first(
            &mut agg.client_version,
            value.get("version").and_then(Value::as_str),
            "version",
        );
        capture_first(
            &mut agg.entrypoint,
            value.get("entrypoint").and_then(Value::as_str),
            "entrypoint",
        );
        capture_first(
            &mut agg.user_type,
            value.get("userType").and_then(Value::as_str),
            "userType",
        );
        capture_first(
            &mut agg.slug,
            value.get("slug").and_then(Value::as_str),
            "slug",
        );
        capture_first(
            &mut agg.session_id,
            value.get("sessionId").and_then(Value::as_str),
            "sessionId",
        );

        if let Some(ts_ms) = parse_iso_timestamp(value.get("timestamp")) {
            agg.first_ts_ms = Some(agg.first_ts_ms.map_or(ts_ms, |f| f.min(ts_ms)));
            agg.last_ts_ms = Some(agg.last_ts_ms.map_or(ts_ms, |l| l.max(ts_ms)));
        }

        match value.get("type").and_then(Value::as_str).unwrap_or("") {
            "user" => {
                handle_claude_user_record(&mut agg, &value, idx);
            }
            "assistant" => {
                saw_any_assistant = true;
                handle_claude_assistant_record(&mut agg, &value);
            }
            "attachment" => {
                handle_claude_attachment(&mut agg, &value);
            }
            "ai-title" => {
                if let Some(title) = value.get("aiTitle").and_then(Value::as_str) {
                    if agg.ai_title.is_none() && !title.is_empty() {
                        agg.ai_title = Some(title.to_string());
                    }
                }
            }
            "permission-mode" => {
                if let Some(mode) = value.get("permissionMode").and_then(Value::as_str) {
                    agg.permission_mode = Some(mode.to_string());
                    agg.permission_modes_seen.insert(mode.to_string());
                }
            }
            "last-prompt" | "queue-operation" => {
                // Marker records — no usage data, just a pointer or
                // queue operation. We surface their existence via the
                // `attachments_seen` extras blob below would be misleading
                // (they're not attachments), so we just skip them.
            }
            _ => {}
        }

        if value.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            agg.sidechain_turn_count += 1;
        }
    }

    if !saw_any_assistant && agg.input_tokens == 0 && agg.output_tokens == 0 {
        return None;
    }
    Some(agg)
}

fn handle_claude_user_record(agg: &mut TranscriptAggregate, value: &Value, line_idx: usize) {
    if let Some(mode) = value.get("permissionMode").and_then(Value::as_str) {
        agg.permission_modes_seen.insert(mode.to_string());
        agg.permission_mode = Some(mode.to_string());
    }
    let message = value.get("message").unwrap_or(value);
    if message.get("role").and_then(Value::as_str) != Some("user") {
        return;
    }
    let content = message.get("content");
    let Some(content) = content else { return };

    let mut prompt_text = String::new();

    if let Some(text) = content.as_str() {
        prompt_text.push_str(text);
    } else if let Some(arr) = content.as_array() {
        let mut is_tool_result = false;
        for block in arr {
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(t) = block.get("text").and_then(Value::as_str) {
                        if !prompt_text.is_empty() {
                            prompt_text.push('\n');
                        }
                        prompt_text.push_str(t);
                    }
                }
                Some("tool_result") => {
                    is_tool_result = true;
                    // Tool results aren't user prompts. Look for
                    // `is_error` to bump our error counter.
                    if block.get("is_error").and_then(Value::as_bool) == Some(true) {
                        agg.error_count += 1;
                    }
                    if block.get("interrupted").and_then(Value::as_bool) == Some(true) {
                        agg.interrupted_tool_count += 1;
                    }
                }
                _ => {}
            }
        }
        if is_tool_result {
            return;
        }
    }

    if !prompt_text.trim().is_empty() {
        agg.turn_count += 1;
        let ts = parse_iso_timestamp(value.get("timestamp"));
        agg.prompts.push(PromptRecord {
            sequence: line_idx as i64,
            text: prompt_text,
            has_paste: false,
            timestamp_ms: ts,
        });
    }
}

fn handle_claude_assistant_record(agg: &mut TranscriptAggregate, value: &Value) {
    let message = value.get("message").unwrap_or(value);

    if let Some(model) = message.get("model").and_then(Value::as_str) {
        if agg.model.is_none() && !model.is_empty() {
            agg.model = Some(model.to_string());
        }
    }
    if let Some(reason) = message.get("stop_reason").and_then(Value::as_str) {
        agg.stop_reasons.insert(reason.to_string());
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

        // Prefer the 5m/1h breakdown when present; fall back to the flat
        // `cache_creation_input_tokens` (treated as 5m) for older records.
        let flat_cache = usage
            .get("cache_creation_input_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        if let Some(split) = usage.get("cache_creation").and_then(Value::as_object) {
            let m5 = split
                .get("ephemeral_5m_input_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let h1 = split
                .get("ephemeral_1h_input_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            agg.cache_5m_tokens += m5;
            agg.cache_1h_tokens += h1;
            // If the split is all zero but the flat field has data, fall back.
            if m5 == 0 && h1 == 0 {
                agg.cache_5m_tokens += flat_cache;
            }
        } else {
            agg.cache_5m_tokens += flat_cache;
        }

        if let Some(server_tools) = usage.get("server_tool_use").and_then(Value::as_object) {
            agg.web_search_requests += server_tools
                .get("web_search_requests")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            agg.web_fetch_requests += server_tools
                .get("web_fetch_requests")
                .and_then(Value::as_i64)
                .unwrap_or(0);
        }

        if agg.service_tier.is_none() {
            if let Some(tier) = usage.get("service_tier").and_then(Value::as_str) {
                if !tier.is_empty() {
                    agg.service_tier = Some(tier.to_string());
                }
            }
        }
        if agg.speed.is_none() {
            if let Some(speed) = usage.get("speed").and_then(Value::as_str) {
                if !speed.is_empty() {
                    agg.speed = Some(speed.to_string());
                }
            }
        }
        if agg.inference_geo.is_none() {
            if let Some(geo) = usage.get("inference_geo").and_then(Value::as_str) {
                if !geo.is_empty() {
                    agg.inference_geo = Some(geo.to_string());
                }
            }
        }

        if let Some(iters) = usage.get("iterations").and_then(Value::as_array) {
            agg.iteration_count += iters.len() as i64;
        }
    }

    if let Some(content) = message.get("content").and_then(Value::as_array) {
        for block in content {
            match block.get("type").and_then(Value::as_str) {
                Some("tool_use") => {
                    agg.tool_call_count += 1;
                    if let Some(name) = block.get("name").and_then(Value::as_str) {
                        agg.tools.insert(name.to_string());
                        if let Some(rest) = name.strip_prefix("mcp__") {
                            if let Some((server, _)) = rest.split_once("__") {
                                agg.mcp_servers.insert(server.to_string());
                            } else {
                                agg.mcp_servers.insert(rest.to_string());
                            }
                        }
                        if name == "Skill" {
                            if let Some(skill) = block
                                .get("input")
                                .and_then(|i| i.get("skill"))
                                .and_then(Value::as_str)
                            {
                                agg.skills.insert(skill.to_string());
                            }
                        }
                    }
                }
                Some("server_tool_use") => {
                    // Server-side tool calls (web_search, web_fetch).
                    // We bill them via the `usage.server_tool_use`
                    // counter; surface the names in `tools` for visibility.
                    if let Some(name) = block.get("name").and_then(Value::as_str) {
                        agg.tools.insert(name.to_string());
                    }
                }
                _ => {}
            }
        }
    }
}

fn handle_claude_attachment(agg: &mut TranscriptAggregate, value: &Value) {
    let Some(att) = value.get("attachment") else {
        return;
    };
    let kind = att.get("type").and_then(Value::as_str).unwrap_or("");
    if !kind.is_empty() {
        agg.attachments_seen.insert(kind.to_string());
    }

    match kind {
        "hook_success" => {
            let hook_event = att.get("hookEvent").and_then(Value::as_str).unwrap_or("");
            let hook_name = att.get("hookName").and_then(Value::as_str).unwrap_or("");
            let key = if hook_name.is_empty() {
                hook_event.to_string()
            } else {
                format!("{hook_event}:{hook_name}")
            };
            let duration_ms = att.get("durationMs").and_then(Value::as_i64).unwrap_or(0);
            let exit_code = att.get("exitCode").and_then(Value::as_i64).unwrap_or(0);
            let entry = agg.hook_executions.entry(key).or_default();
            entry.count += 1;
            entry.total_ms += duration_ms;
            if exit_code != 0 {
                entry.error_count += 1;
            }
        }
        "skill_listing" => {
            // Initial skill listing — surfaces what was *available*, not
            // what got invoked. We track invocations via `Skill` tool_use.
        }
        "plan_mode" => {
            agg.plan_mode_used = true;
        }
        _ => {}
    }
}

// =============================================================================
// Codex transcript parser
// =============================================================================

/// Parse a Codex CLI session JSONL file. Two formats coexist on disk:
///
/// **Newer format** emits explicit `session_meta` / `turn_context` /
/// `token_count` events with cumulative usage totals (we diff successive
/// totals to avoid double-counting).
///
/// **Older format** has no token events at all — only `message`,
/// `reasoning`, `function_call`, and `function_call_output`. We extract
/// what we can: cwd/approval_policy/sandbox from the
/// `<environment_context>` text inside the first user message, plus
/// turn/tool/reasoning counts and shell durations.
fn parse_codex_transcript(raw: &str) -> Option<TranscriptAggregate> {
    let mut agg = TranscriptAggregate::default();
    let mut previous_totals: Option<(i64, i64, i64)> = None;
    let mut saw_anything = false;
    let mut line_idx: i64 = -1;

    for line in raw.lines() {
        line_idx += 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        saw_anything = true;

        // Session-header line (first record): {id, timestamp, instructions}.
        if let Some(id) = value.get("id").and_then(Value::as_str) {
            if agg.session_id.is_none() {
                agg.session_id = Some(id.to_string());
            }
        }
        if value.get("instructions").is_some()
            && !value.get("instructions").unwrap_or(&Value::Null).is_null()
        {
            agg.instructions_present = true;
        }
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
                    if agg.cwd.is_none() {
                        agg.cwd = Some(cwd);
                    }
                }
                if let Some(sid) = pick_field_str(payload, &["id", "session_id", "sessionId"]) {
                    if agg.session_id.is_none() {
                        agg.session_id = Some(sid);
                    }
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
                if let Some(policy) = pick_field_str(payload, &["approval_policy"]) {
                    agg.approval_policy = Some(policy);
                }
                if let Some(mode) = pick_field_str(payload, &["sandbox_mode"]) {
                    agg.sandbox_mode = Some(mode);
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
            "message" => handle_codex_message(&mut agg, payload, line_idx),
            "reasoning" => {
                agg.reasoning_count += 1;
            }
            "function_call" => handle_codex_function_call(&mut agg, payload),
            "function_call_output" => handle_codex_function_call_output(&mut agg, payload),
            _ => {}
        }
    }

    if !saw_anything {
        return None;
    }
    Some(agg)
}

fn handle_codex_message(agg: &mut TranscriptAggregate, payload: &Value, line_idx: i64) {
    let role = payload.get("role").and_then(Value::as_str).unwrap_or("");
    let content = payload.get("content").and_then(Value::as_array);
    if role == "user" {
        let mut text = String::new();
        if let Some(blocks) = content {
            for b in blocks {
                if let Some(t) = b.get("text").and_then(Value::as_str) {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(t);
                }
            }
        }
        if text.contains("<environment_context>") {
            parse_codex_environment_context(agg, &text);
        }
        let stripped = strip_environment_context(&text);
        if !stripped.trim().is_empty() {
            agg.turn_count += 1;
            agg.prompts.push(PromptRecord {
                sequence: line_idx,
                text: stripped,
                has_paste: false,
                timestamp_ms: None,
            });
        }
    } else if role == "assistant" {
        // No-op for cost; we count tool calls below via function_call.
    }
}

fn parse_codex_environment_context(agg: &mut TranscriptAggregate, text: &str) {
    // The block looks like:
    //   <environment_context>
    //   Current working directory: C:\Users\georg
    //   Approval policy: on-request
    //   Sandbox mode: workspace-write
    //   Network access: restricted
    //   </environment_context>
    for line in text.lines() {
        let line = line.trim();
        if let Some(value) = strip_prefix_ci(line, "Current working directory:") {
            if agg.cwd.is_none() {
                agg.cwd = Some(value.trim().to_string());
            }
        } else if let Some(value) = strip_prefix_ci(line, "Approval policy:") {
            if agg.approval_policy.is_none() {
                agg.approval_policy = Some(value.trim().to_string());
            }
        } else if let Some(value) = strip_prefix_ci(line, "Sandbox mode:") {
            if agg.sandbox_mode.is_none() {
                agg.sandbox_mode = Some(value.trim().to_string());
            }
        } else if let Some(value) = strip_prefix_ci(line, "Network access:") {
            if agg.network_access.is_none() {
                agg.network_access = Some(value.trim().to_string());
            }
        }
    }
}

fn strip_environment_context(text: &str) -> String {
    // Strip a single <environment_context>...</environment_context> block
    // including the surrounding whitespace. Keeps everything else.
    if let (Some(start), Some(end)) = (
        text.find("<environment_context>"),
        text.find("</environment_context>"),
    ) {
        if start < end {
            let mut out = String::new();
            out.push_str(&text[..start]);
            out.push_str(&text[end + "</environment_context>".len()..]);
            return out.trim().to_string();
        }
    }
    text.to_string()
}

fn handle_codex_function_call(agg: &mut TranscriptAggregate, payload: &Value) {
    let name = payload
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if !name.is_empty() {
        agg.codex_function_calls.insert(name.clone());
        agg.tools.insert(name.clone());
        agg.tool_call_count += 1;

        // Heuristic: a `shell` call with `with_escalated_permissions=true`
        // counts as an escalation. Codex emits `arguments` as a JSON
        // string, so we re-parse it.
        if let Some(args_str) = payload.get("arguments").and_then(Value::as_str) {
            if let Ok(args) = serde_json::from_str::<Value>(args_str) {
                if args
                    .get("with_escalated_permissions")
                    .and_then(Value::as_bool)
                    == Some(true)
                {
                    agg.escalated_permission_count += 1;
                }
            }
        }

        if name == "update_plan" {
            agg.plan_mode_used = true;
        }
    }
}

fn handle_codex_function_call_output(agg: &mut TranscriptAggregate, payload: &Value) {
    if let Some(output_str) = payload.get("output").and_then(Value::as_str) {
        if let Ok(parsed) = serde_json::from_str::<Value>(output_str) {
            let exit = parsed
                .get("metadata")
                .and_then(|m| m.get("exit_code"))
                .and_then(Value::as_i64);
            if exit.is_some_and(|c| c != 0) {
                agg.error_count += 1;
            }
            let secs = parsed
                .get("metadata")
                .and_then(|m| m.get("duration_seconds"))
                .and_then(Value::as_f64)
                .unwrap_or(0.0);
            agg.codex_total_shell_secs += secs.round() as i64;
        }
    }
}

// =============================================================================
// Copilot CLI transcript parser
// =============================================================================

/// Parse a Copilot CLI session events.jsonl. Captures branch/cwd from
/// `session.start`, model from `tool.execution_complete`, and turn /
/// tool counts. Copilot does not expose token usage; cost is always 0.
fn parse_copilot_transcript(raw: &str) -> Option<TranscriptAggregate> {
    let mut agg = TranscriptAggregate::default();
    let mut saw_anything = false;
    let mut line_idx: i64 = -1;

    for line in raw.lines() {
        line_idx += 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        saw_anything = true;

        if let Some(ts_ms) = parse_iso_timestamp(value.get("timestamp")) {
            agg.first_ts_ms = Some(agg.first_ts_ms.map_or(ts_ms, |f| f.min(ts_ms)));
            agg.last_ts_ms = Some(agg.last_ts_ms.map_or(ts_ms, |l| l.max(ts_ms)));
        }

        let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
        let data = value.get("data").unwrap_or(&Value::Null);

        match kind {
            "session.start" => {
                if let Some(sid) = data.get("sessionId").and_then(Value::as_str) {
                    agg.session_id = Some(sid.to_string());
                }
                if let Some(ver) = data.get("copilotVersion").and_then(Value::as_str) {
                    agg.client_version = Some(ver.to_string());
                }
                if let Some(ctx) = data.get("context") {
                    if let Some(cwd) = ctx.get("cwd").and_then(Value::as_str) {
                        agg.cwd = Some(cwd.to_string());
                    }
                    if let Some(branch) = ctx.get("branch").and_then(Value::as_str) {
                        agg.git_branch = Some(branch.to_string());
                    }
                }
                if let Some(start) = data
                    .get("startTime")
                    .and_then(|v| parse_iso_timestamp(Some(v)))
                {
                    agg.first_ts_ms = Some(agg.first_ts_ms.map_or(start, |f| f.min(start)));
                }
            }
            "session.error" => {
                agg.error_count += 1;
                let msg = data
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if !msg.is_empty() {
                    agg.copilot_error = Some(msg);
                }
            }
            "user.message" => {
                agg.turn_count += 1;
                if let Some(content) = data.get("content").and_then(Value::as_str) {
                    if !content.trim().is_empty() {
                        let attachments = data.get("attachments").and_then(Value::as_array);
                        let has_paste = attachments.is_some_and(|a| !a.is_empty());
                        agg.prompts.push(PromptRecord {
                            sequence: line_idx,
                            text: content.to_string(),
                            has_paste,
                            timestamp_ms: parse_iso_timestamp(value.get("timestamp")),
                        });
                    }
                }
            }
            "assistant.turn_start" => {
                // Already counted via user.message; nothing to do.
            }
            "assistant.message" => {
                if let Some(reqs) = data.get("toolRequests").and_then(Value::as_array) {
                    for r in reqs {
                        if let Some(name) = r.get("name").and_then(Value::as_str) {
                            agg.tools.insert(name.to_string());
                        }
                    }
                }
            }
            "tool.execution_start" => {
                agg.tool_call_count += 1;
                if let Some(name) = data.get("toolName").and_then(Value::as_str) {
                    agg.tools.insert(name.to_string());
                }
            }
            "tool.execution_complete" => {
                if let Some(model) = data.get("model").and_then(Value::as_str) {
                    if agg.model.is_none() {
                        agg.model = Some(model.to_string());
                    }
                }
                if data.get("success").and_then(Value::as_bool) == Some(false) {
                    agg.error_count += 1;
                }
            }
            "subagent.started" => {
                agg.subagent_count += 1;
                if let Some(name) = data.get("agentName").and_then(Value::as_str) {
                    agg.copilot_subagents.insert(name.to_string());
                }
            }
            _ => {}
        }
    }

    if !saw_anything {
        return None;
    }
    Some(agg)
}

// =============================================================================
// History-stream ingestion (~/.claude/history.jsonl, ~/.codex/history.jsonl,
// ~/.copilot/command-history-state.json)
// =============================================================================

fn ingest_claude_history(state: &mut ProcessedState) {
    let Some(path) = home_dir().map(|h| h.join(".claude").join("history.jsonl")) else {
        return;
    };
    if !path.is_file() {
        return;
    }
    ingest_jsonl_history(state, &path, "claude", parse_claude_history_line);
}

fn ingest_codex_history(state: &mut ProcessedState) {
    let Some(path) = home_dir().map(|h| h.join(".codex").join("history.jsonl")) else {
        return;
    };
    if !path.is_file() {
        return;
    }
    ingest_jsonl_history(state, &path, "codex", parse_codex_history_line);
}

fn ingest_copilot_history(state: &mut ProcessedState) {
    let Some(path) = home_dir().map(|h| h.join(".copilot").join("command-history-state.json"))
    else {
        return;
    };
    if !path.is_file() {
        return;
    }
    let key = path.to_string_lossy().to_string();
    let watermark = state.history_watermarks.get(&key).copied().unwrap_or(0);
    let len = file_len(&path);
    if len <= watermark {
        return;
    }

    // Copilot's history is a single JSON object with `commandHistory: []`.
    // Re-parse the whole thing each time; idempotency is handled by the
    // unique index on (provider, source_file, sequence).
    let Ok(raw) = fs::read_to_string(&path) else {
        return;
    };
    let Ok(parsed) = serde_json::from_str::<Value>(&raw) else {
        return;
    };
    let Some(items) = parsed.get("commandHistory").and_then(Value::as_array) else {
        return;
    };

    let Ok(conn) = crate::models::db::write_conn() else {
        return;
    };
    let _ = conn.execute_batch("BEGIN IMMEDIATE");
    for (idx, value) in items.iter().enumerate() {
        let Some(text) = value.as_str() else { continue };
        if text.trim().is_empty() {
            continue;
        }
        let _ = ai_prompts::insert_with_conn(
            &conn,
            AiPromptInsert {
                provider: "copilot".into(),
                source: "history".into(),
                source_file: Some(key.clone()),
                sequence: idx as i64,
                session_id: None,
                workspace_id: None,
                repo_id: None,
                project_path: None,
                git_branch: None,
                prompt: text.to_string(),
                has_paste: false,
                timestamp_ms: None,
            },
        );
    }
    let _ = conn.execute_batch("COMMIT");
    state.history_watermarks.insert(key, len);
}

#[derive(Debug, Default)]
struct HistoryRecord {
    text: String,
    timestamp_ms: Option<u64>,
    project: Option<String>,
    session_id: Option<String>,
    has_paste: bool,
}

fn parse_claude_history_line(line: &str) -> Option<HistoryRecord> {
    let value: Value = serde_json::from_str(line).ok()?;
    let text = value.get("display").and_then(Value::as_str)?.to_string();
    if text.trim().is_empty() {
        return None;
    }
    let timestamp_ms = value
        .get("timestamp")
        .and_then(Value::as_i64)
        .map(|v| v as u64);
    let project = value
        .get("project")
        .and_then(Value::as_str)
        .map(String::from);
    // pastedContents is sometimes an empty object even when nothing was
    // pasted; treat any non-empty object as a paste.
    let has_paste = value
        .get("pastedContents")
        .map(|v| v.is_object() && !v.as_object().unwrap().is_empty())
        .unwrap_or(false);
    Some(HistoryRecord {
        text,
        timestamp_ms,
        project,
        session_id: None,
        has_paste,
    })
}

fn parse_codex_history_line(line: &str) -> Option<HistoryRecord> {
    let value: Value = serde_json::from_str(line).ok()?;
    let text = value.get("text").and_then(Value::as_str)?.to_string();
    if text.trim().is_empty() {
        return None;
    }
    // Codex stores Unix seconds, not millis.
    let timestamp_ms = value
        .get("ts")
        .and_then(Value::as_i64)
        .map(|s| (s as u64) * 1000);
    let session_id = value
        .get("session_id")
        .and_then(Value::as_str)
        .map(String::from);
    Some(HistoryRecord {
        text,
        timestamp_ms,
        project: None,
        session_id,
        has_paste: false,
    })
}

fn ingest_jsonl_history(
    state: &mut ProcessedState,
    path: &Path,
    provider: &str,
    parse_line: fn(&str) -> Option<HistoryRecord>,
) {
    let key = path.to_string_lossy().to_string();
    let watermark = state.history_watermarks.get(&key).copied().unwrap_or(0);
    let len = file_len(path);
    if len <= watermark {
        return;
    }

    // Read the whole file (cheap — these are append-only and ≪ 100 MB
    // even for heavy users) and walk line-by-line. Sequence is the
    // 0-indexed line number, so re-scans are idempotent via the unique
    // index even if we skip a file partway through.
    let Ok(raw) = fs::read_to_string(path) else {
        return;
    };
    let Ok(conn) = crate::models::db::write_conn() else {
        return;
    };
    let _ = conn.execute_batch("BEGIN IMMEDIATE");
    for (idx, line) in raw.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some(rec) = parse_line(trimmed) else {
            continue;
        };
        let (workspace_id, repo_id, _) = attribute_session(rec.project.as_deref(), None, None);
        let _ = ai_prompts::insert_with_conn(
            &conn,
            AiPromptInsert {
                provider: provider.into(),
                source: "history".into(),
                source_file: Some(key.clone()),
                sequence: idx as i64,
                session_id: rec.session_id,
                workspace_id,
                repo_id,
                project_path: rec.project,
                git_branch: None,
                prompt: rec.text,
                has_paste: rec.has_paste,
                timestamp_ms: rec.timestamp_ms.map(|m| m as i64),
            },
        );
    }
    let _ = conn.execute_batch("COMMIT");
    state.history_watermarks.insert(key, len);
}

// =============================================================================
// Helpers
// =============================================================================

fn capture_first(slot: &mut Option<String>, candidate: Option<&str>, _label: &str) {
    if slot.is_some() {
        return;
    }
    if let Some(v) = candidate {
        if !v.is_empty() {
            *slot = Some(v.to_string());
        }
    }
}

fn strip_prefix_ci<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    if line.len() < prefix.len() {
        return None;
    }
    let head = &line[..prefix.len()];
    if head.eq_ignore_ascii_case(prefix) {
        Some(&line[prefix.len()..])
    } else {
        None
    }
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

fn list_recent_jsonl_files(
    root: &Path,
    modified_after: u64,
    only_filename: Option<&str>,
) -> Vec<TranscriptFile> {
    let mut out = Vec::new();
    walk_jsonl(root, &mut out, modified_after, only_filename, 0);
    out
}

fn walk_jsonl(
    dir: &Path,
    out: &mut Vec<TranscriptFile>,
    modified_after: u64,
    only_filename: Option<&str>,
    depth: u32,
) {
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
            walk_jsonl(&path, out, modified_after, only_filename, depth + 1);
        } else if path.extension().is_some_and(|ext| ext == "jsonl") {
            if let Some(target) = only_filename {
                if path.file_name().and_then(|n| n.to_str()) != Some(target) {
                    continue;
                }
            }
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

fn file_len(path: &Path) -> u64 {
    fs::metadata(path).map(|m| m.len()).unwrap_or(0)
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

fn copilot_transcripts_root() -> Option<PathBuf> {
    let home = home_dir()?;
    Some(home.join(".copilot").join("session-state"))
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

/// Resolve `(workspace_id, repo_id, is_pr_create)` from cwd, session_id,
/// and gitBranch. Best-effort: missing matches just leave fields null.
///
/// Order of preference:
///   1. session_id directly (most precise — joins through `sessions`).
///   2. cwd + git_branch combined LIKE match against workspaces.
///   3. cwd alone via the original directory_name LIKE heuristic.
fn attribute_session(
    cwd: Option<&str>,
    session_id: Option<&str>,
    git_branch: Option<&str>,
) -> (Option<String>, Option<String>, bool) {
    let Ok(conn) = crate::models::db::read_conn() else {
        return (None, None, false);
    };

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

    // Try gitBranch — if there's exactly one workspace on that branch,
    // attribution is unambiguous regardless of cwd.
    if let Some(branch) = git_branch {
        if let Ok((wid, rid)) = conn.query_row(
            "SELECT id, repository_id FROM workspaces WHERE branch = ?1
             ORDER BY updated_at DESC LIMIT 1",
            rusqlite::params![branch],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        ) {
            return (Some(wid), rid, false);
        }
    }

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
        let raw = r#"{"type":"user","timestamp":"2025-10-20T10:00:00Z","cwd":"/work/repo","gitBranch":"feat/x","version":"2.1.0","entrypoint":"cli","userType":"external","sessionId":"sess-1","permissionMode":"bypassPermissions","message":{"role":"user","content":"hi there"}}
{"type":"assistant","timestamp":"2025-10-20T10:00:01Z","cwd":"/work/repo","gitBranch":"feat/x","version":"2.1.0","sessionId":"sess-1","message":{"role":"assistant","model":"claude-sonnet-4-5","stop_reason":"end_turn","usage":{"input_tokens":1000,"output_tokens":500,"cache_read_input_tokens":2000,"cache_creation_input_tokens":300,"cache_creation":{"ephemeral_5m_input_tokens":100,"ephemeral_1h_input_tokens":200},"server_tool_use":{"web_search_requests":1,"web_fetch_requests":2},"service_tier":"standard","speed":"standard","iterations":[{},{}]},"content":[{"type":"tool_use","name":"Bash","input":{"command":"ls"}},{"type":"tool_use","name":"Skill","input":{"skill":"review"}}]}}
{"type":"ai-title","aiTitle":"Help with X","sessionId":"sess-1"}
{"type":"attachment","timestamp":"2025-10-20T10:00:02Z","attachment":{"type":"hook_success","hookEvent":"PostToolUse","hookName":"Bash","durationMs":120,"exitCode":0,"command":"echo ok"}}
{"type":"attachment","timestamp":"2025-10-20T10:00:03Z","attachment":{"type":"hook_success","hookEvent":"PostToolUse","hookName":"Bash","durationMs":50,"exitCode":1,"command":"err"}}
{"type":"attachment","timestamp":"2025-10-20T10:00:04Z","attachment":{"type":"plan_mode","planExists":true,"planFilePath":"/x"}}
{"type":"assistant","timestamp":"2025-10-20T10:01:00Z","sessionId":"sess-1","message":{"role":"assistant","model":"claude-sonnet-4-5","stop_reason":"tool_use","usage":{"input_tokens":50,"output_tokens":100,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":0}},"content":[{"type":"tool_use","name":"mcp__github__create_pr","input":{}}]}}"#;

        let agg = parse_claude_transcript(raw).unwrap();
        assert_eq!(agg.input_tokens, 1050);
        assert_eq!(agg.output_tokens, 600);
        assert_eq!(agg.cache_read_tokens, 2000);
        assert_eq!(agg.cache_5m_tokens, 100);
        assert_eq!(agg.cache_1h_tokens, 200);
        assert_eq!(agg.web_search_requests, 1);
        assert_eq!(agg.web_fetch_requests, 2);
        assert_eq!(agg.service_tier.as_deref(), Some("standard"));
        assert_eq!(agg.speed.as_deref(), Some("standard"));
        assert_eq!(agg.iteration_count, 2);
        assert_eq!(agg.model.as_deref(), Some("claude-sonnet-4-5"));
        assert_eq!(agg.cwd.as_deref(), Some("/work/repo"));
        assert_eq!(agg.git_branch.as_deref(), Some("feat/x"));
        assert_eq!(agg.client_version.as_deref(), Some("2.1.0"));
        assert_eq!(agg.entrypoint.as_deref(), Some("cli"));
        assert_eq!(agg.user_type.as_deref(), Some("external"));
        assert_eq!(agg.session_id.as_deref(), Some("sess-1"));
        assert_eq!(agg.ai_title.as_deref(), Some("Help with X"));
        assert_eq!(agg.permission_mode.as_deref(), Some("bypassPermissions"));
        assert!(agg.tools.contains("Bash"));
        assert!(agg.tools.contains("Skill"));
        assert!(agg.tools.contains("mcp__github__create_pr"));
        assert!(agg.mcp_servers.contains("github"));
        assert!(agg.skills.contains("review"));
        assert!(agg.stop_reasons.contains("end_turn"));
        assert!(agg.stop_reasons.contains("tool_use"));
        assert_eq!(agg.tool_call_count, 3);
        assert_eq!(agg.turn_count, 1);
        assert!(agg.plan_mode_used);
        let bash_hooks = agg
            .hook_executions
            .get("PostToolUse:Bash")
            .expect("hook stat");
        assert_eq!(bash_hooks.count, 2);
        assert_eq!(bash_hooks.total_ms, 170);
        assert_eq!(bash_hooks.error_count, 1);
        assert_eq!(agg.prompts.len(), 1);
        assert_eq!(agg.prompts[0].text, "hi there");
    }

    #[test]
    fn parse_claude_transcript_falls_back_when_split_is_zero() {
        // When `cache_creation: {5m:0, 1h:0}` is present alongside a
        // non-zero flat field, we should treat the flat as 5m.
        let raw = r#"{"type":"assistant","timestamp":"2025-10-20T10:00:00Z","sessionId":"s","message":{"role":"assistant","model":"claude-sonnet-4-5","stop_reason":"end_turn","usage":{"input_tokens":0,"output_tokens":1,"cache_read_input_tokens":0,"cache_creation_input_tokens":500,"cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":0}},"content":[]}}"#;
        let agg = parse_claude_transcript(raw).unwrap();
        assert_eq!(agg.cache_5m_tokens, 500);
        assert_eq!(agg.cache_1h_tokens, 0);
    }

    #[test]
    fn parse_codex_transcript_uses_total_deltas() {
        let raw = r#"{"timestamp":"2025-10-20T10:00:00Z","type":"session_meta","payload":{"cwd":"/work/repo","id":"sess-1"}}
{"timestamp":"2025-10-20T10:00:01Z","type":"turn_context","payload":{"model":"gpt-5","approval_policy":"on-request","sandbox_mode":"workspace-write"}}
{"timestamp":"2025-10-20T10:00:10Z","type":"token_count","payload":{"info":{"total_token_usage":{"input_tokens":500,"output_tokens":100,"cached_input_tokens":0}}}}
{"timestamp":"2025-10-20T10:00:30Z","type":"token_count","payload":{"info":{"total_token_usage":{"input_tokens":1500,"output_tokens":400,"cached_input_tokens":200}}}}"#;

        let agg = parse_codex_transcript(raw).unwrap();
        assert_eq!(agg.input_tokens, 1500);
        assert_eq!(agg.output_tokens, 400);
        assert_eq!(agg.cache_read_tokens, 200);
        assert_eq!(agg.model.as_deref(), Some("gpt-5"));
        assert_eq!(agg.cwd.as_deref(), Some("/work/repo"));
        assert_eq!(agg.session_id.as_deref(), Some("sess-1"));
        assert_eq!(agg.approval_policy.as_deref(), Some("on-request"));
        assert_eq!(agg.sandbox_mode.as_deref(), Some("workspace-write"));
    }

    #[test]
    fn parse_codex_older_format_extracts_environment_context() {
        // Pre-token_count Codex sessions only have message/reasoning/
        // function_call. We should still pull cwd/approval/sandbox out of
        // the user message that embeds <environment_context>.
        let raw = r#"{"id":"4ba5f133","timestamp":"2026-04-11T10:10:40Z","instructions":null}
{"record_type":"state"}
{"type":"message","id":null,"role":"user","content":[{"type":"input_text","text":"<environment_context>\nCurrent working directory: C:\\Users\\georg\nApproval policy: on-request\nSandbox mode: workspace-write\nNetwork access: restricted\n</environment_context>"}]}
{"type":"message","id":null,"role":"user","content":[{"type":"input_text","text":"review the code in C:/work/levi"}]}
{"type":"reasoning","id":"r1","summary":[],"content":null}
{"type":"function_call","id":"fc1","name":"shell","arguments":"{\"command\":[\"bash\",\"-lc\",\"ls\"],\"with_escalated_permissions\":true}","call_id":"call_1"}
{"type":"function_call_output","call_id":"call_1","output":"{\"output\":\"\",\"metadata\":{\"exit_code\":1,\"duration_seconds\":2.8}}"}"#;
        let agg = parse_codex_transcript(raw).unwrap();
        assert_eq!(agg.session_id.as_deref(), Some("4ba5f133"));
        assert_eq!(agg.cwd.as_deref(), Some("C:\\Users\\georg"));
        assert_eq!(agg.approval_policy.as_deref(), Some("on-request"));
        assert_eq!(agg.sandbox_mode.as_deref(), Some("workspace-write"));
        assert_eq!(agg.network_access.as_deref(), Some("restricted"));
        assert_eq!(agg.reasoning_count, 1);
        assert_eq!(agg.tool_call_count, 1);
        assert_eq!(agg.escalated_permission_count, 1);
        assert_eq!(agg.error_count, 1); // exit_code != 0
        assert!(agg.codex_function_calls.contains("shell"));
        assert!(agg.codex_total_shell_secs >= 2);
        assert!(!agg.instructions_present); // null
                                            // Two user messages, one is environment_context only (stripped).
        assert_eq!(agg.turn_count, 1);
        assert_eq!(agg.prompts.len(), 1);
        assert!(agg.prompts[0]
            .text
            .contains("review the code in C:/work/levi"));
    }

    #[test]
    fn parse_copilot_transcript_collects_session_metadata() {
        let raw = r#"{"type":"session.start","data":{"sessionId":"abc","version":1,"copilotVersion":"0.0.421","startTime":"2026-03-05T22:57:11.624Z","context":{"cwd":"C:\\Work\\SlimWin","gitRoot":"C:\\Work\\SlimWin","branch":"master"}},"timestamp":"2026-03-05T22:57:11.683Z"}
{"type":"user.message","data":{"content":"review please","attachments":[]},"timestamp":"2026-03-05T23:08:34.887Z"}
{"type":"tool.execution_start","data":{"toolCallId":"c1","toolName":"report_intent","arguments":{}},"timestamp":"2026-03-05T23:08:35Z"}
{"type":"tool.execution_complete","data":{"toolCallId":"c1","model":"gpt-4.1","success":true,"result":{}},"timestamp":"2026-03-05T23:08:36Z"}
{"type":"subagent.started","data":{"agentName":"explore","agentDisplayName":"Explore"},"timestamp":"2026-03-05T23:08:37Z"}"#;
        let agg = parse_copilot_transcript(raw).unwrap();
        assert_eq!(agg.session_id.as_deref(), Some("abc"));
        assert_eq!(agg.client_version.as_deref(), Some("0.0.421"));
        assert_eq!(agg.cwd.as_deref(), Some("C:\\Work\\SlimWin"));
        assert_eq!(agg.git_branch.as_deref(), Some("master"));
        assert_eq!(agg.model.as_deref(), Some("gpt-4.1"));
        assert_eq!(agg.turn_count, 1);
        assert_eq!(agg.tool_call_count, 1);
        assert_eq!(agg.subagent_count, 1);
        assert!(agg.copilot_subagents.contains("explore"));
        assert!(agg.tools.contains("report_intent"));
        assert_eq!(agg.prompts.len(), 1);
        assert_eq!(agg.prompts[0].text, "review please");
    }

    #[test]
    fn parse_claude_history_record() {
        let line = r#"{"display":"hello world","pastedContents":{"a":"b"},"timestamp":1759084982231,"project":"C:\\Work\\foo"}"#;
        let rec = parse_claude_history_line(line).unwrap();
        assert_eq!(rec.text, "hello world");
        assert_eq!(rec.timestamp_ms, Some(1759084982231));
        assert_eq!(rec.project.as_deref(), Some("C:\\Work\\foo"));
        assert!(rec.has_paste);
    }

    #[test]
    fn parse_codex_history_record_converts_seconds_to_millis() {
        let line = r#"{"session_id":"a1","ts":1756057252,"text":"clone github.com"}"#;
        let rec = parse_codex_history_line(line).unwrap();
        assert_eq!(rec.text, "clone github.com");
        assert_eq!(rec.timestamp_ms, Some(1_756_057_252_000));
        assert_eq!(rec.session_id.as_deref(), Some("a1"));
    }

    #[test]
    fn strip_environment_context_removes_block() {
        let input = "<environment_context>\ncwd: x\n</environment_context>\n\nactual prompt";
        assert_eq!(strip_environment_context(input), "actual prompt");
    }
}
