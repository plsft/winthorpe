//! Per-turn token + cost ledger for agent activity.
//!
//! Schema mirrors worktale's `ai_sessions` (github.com/worktale) so a
//! future "export to worktale" sync is a straight column copy. Each row
//! represents one agent turn — one prompt that resulted in one final
//! response, with all the tool calls, tokens, and cost rolled up.
//!
//! Reads aggregate via SUM at query time (cost-per-session,
//! cost-per-workspace). The volumes are small enough — even a heavy user
//! generates < 1k turns/week — that we don't need a materialized rollup.
//!
//! Beyond cost, the row also carries everything the disk transcript
//! exposed about the session: gitBranch, client version, permission mode,
//! Codex sandbox posture, hook execution stats, etc. The hot fields get
//! their own columns; anything we don't promote lives in the `extras`
//! JSON blob so future fields don't require a migration.

use anyhow::{Context, Result};
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::db;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSession {
    pub id: i64,
    pub workspace_id: Option<String>,
    pub session_id: Option<String>,
    pub repo_id: Option<String>,
    pub date: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub tool: Option<String>,
    pub cost_usd: f64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    /// Pre-decoded JSON arrays as strings; the frontend parses them lazily
    /// because most rows in a list view never need the field detail.
    pub tools_used: Option<String>,
    pub mcp_servers: Option<String>,
    pub duration_secs: i64,
    pub commits: Option<String>,
    pub is_pr_create: bool,
    pub note: Option<String>,
    pub timestamp: String,

    // Provenance pulled from the JSONL envelope.
    pub git_branch: Option<String>,
    pub ai_title: Option<String>,
    pub client_version: Option<String>,
    pub entrypoint: Option<String>,
    pub user_type: Option<String>,
    pub slug: Option<String>,
    pub inference_geo: Option<String>,

    // Pricing inputs.
    pub cache_5m_tokens: i64,
    pub cache_1h_tokens: i64,
    pub web_search_requests: i64,
    pub web_fetch_requests: i64,
    pub service_tier: Option<String>,
    pub speed: Option<String>,

    // Behavior counters.
    pub turn_count: i64,
    pub tool_call_count: i64,
    pub sidechain_turn_count: i64,
    pub subagent_count: i64,
    pub iteration_count: i64,
    pub error_count: i64,
    pub interrupted_tool_count: i64,
    pub permission_mode: Option<String>,
    pub stop_reasons: Option<String>,
    pub hook_executions: Option<String>,
    pub skills_used: Option<String>,
    pub plan_mode_used: bool,

    // Codex sandbox posture.
    pub approval_policy: Option<String>,
    pub sandbox_mode: Option<String>,
    pub network_access: Option<String>,
    pub instructions_present: bool,
    pub reasoning_count: i64,
    pub escalated_permission_count: i64,

    pub extras: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSessionInsert {
    pub workspace_id: Option<String>,
    pub session_id: Option<String>,
    pub repo_id: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub tool: Option<String>,
    pub cost_usd: Option<f64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub tools_used: Option<Vec<String>>,
    pub mcp_servers: Option<Vec<String>>,
    pub duration_secs: Option<i64>,
    pub commits: Option<Vec<String>>,
    pub is_pr_create: Option<bool>,
    pub note: Option<String>,

    pub git_branch: Option<String>,
    pub ai_title: Option<String>,
    pub client_version: Option<String>,
    pub entrypoint: Option<String>,
    pub user_type: Option<String>,
    pub slug: Option<String>,
    pub inference_geo: Option<String>,

    pub cache_5m_tokens: Option<i64>,
    pub cache_1h_tokens: Option<i64>,
    pub web_search_requests: Option<i64>,
    pub web_fetch_requests: Option<i64>,
    pub service_tier: Option<String>,
    pub speed: Option<String>,

    pub turn_count: Option<i64>,
    pub tool_call_count: Option<i64>,
    pub sidechain_turn_count: Option<i64>,
    pub subagent_count: Option<i64>,
    pub iteration_count: Option<i64>,
    pub error_count: Option<i64>,
    pub interrupted_tool_count: Option<i64>,
    pub permission_mode: Option<String>,
    pub stop_reasons: Option<Vec<String>>,
    pub hook_executions: Option<serde_json::Value>,
    pub skills_used: Option<Vec<String>>,
    pub plan_mode_used: Option<bool>,

    pub approval_policy: Option<String>,
    pub sandbox_mode: Option<String>,
    pub network_access: Option<String>,
    pub instructions_present: Option<bool>,
    pub reasoning_count: Option<i64>,
    pub escalated_permission_count: Option<i64>,

    pub extras: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AiSessionStats {
    pub total_turns: i64,
    pub total_cost_usd: f64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_cache_read_tokens: i64,
    pub total_cache_write_tokens: i64,
    pub total_duration_secs: i64,
    /// Cost broken out by provider so the UI can render a stacked bar.
    pub cost_by_provider: Vec<ProviderCost>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCost {
    pub provider: String,
    pub cost_usd: f64,
    pub turns: i64,
}

pub fn insert(record: AiSessionInsert) -> Result<i64> {
    insert_with_timestamp(record, None)
}

/// Insert with an explicit session-end timestamp (Unix millis). Used by
/// the transcript scanner so rows show *when the session actually ran*,
/// not when we got around to scanning the file. Falls back to "now" when
/// no timestamp is provided (live recordings from inside a turn).
pub fn insert_with_timestamp(record: AiSessionInsert, timestamp_ms: Option<u64>) -> Result<i64> {
    let conn = db::write_conn()?;
    let (date, override_ts) = match timestamp_ms {
        Some(ms) => {
            let dt = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms as i64)
                .unwrap_or_else(chrono::Utc::now)
                .with_timezone(&chrono::Local);
            (
                dt.format("%Y-%m-%d").to_string(),
                Some(dt.format("%Y-%m-%d %H:%M:%S").to_string()),
            )
        }
        None => (chrono::Local::now().format("%Y-%m-%d").to_string(), None),
    };

    fn json_array(v: &Option<Vec<String>>) -> Option<String> {
        v.as_ref()
            .map(|arr| serde_json::to_string(arr).unwrap_or_else(|_| "[]".into()))
    }
    fn json_value(v: &Option<serde_json::Value>) -> Option<String> {
        v.as_ref()
            .map(|val| serde_json::to_string(val).unwrap_or_else(|_| "{}".into()))
    }

    let tools_json = json_array(&record.tools_used);
    let mcp_json = json_array(&record.mcp_servers);
    let commits_json = json_array(&record.commits);
    let stop_reasons_json = json_array(&record.stop_reasons);
    let skills_json = json_array(&record.skills_used);
    let hooks_json = json_value(&record.hook_executions);
    let extras_json = json_value(&record.extras);

    // The column list is long but explicit — easier to audit than a
    // dynamic builder, and rusqlite will catch arity mismatches at runtime.
    let sql = "INSERT INTO ai_sessions (
        workspace_id, session_id, repo_id, date,
        provider, model, tool,
        cost_usd, input_tokens, output_tokens,
        cache_read_tokens, cache_write_tokens,
        tools_used, mcp_servers,
        duration_secs, commits, is_pr_create, note,
        git_branch, ai_title, client_version, entrypoint, user_type, slug, inference_geo,
        cache_5m_tokens, cache_1h_tokens,
        web_search_requests, web_fetch_requests,
        service_tier, speed,
        turn_count, tool_call_count, sidechain_turn_count, subagent_count,
        iteration_count, error_count, interrupted_tool_count,
        permission_mode, stop_reasons, hook_executions, skills_used, plan_mode_used,
        approval_policy, sandbox_mode, network_access,
        instructions_present, reasoning_count, escalated_permission_count,
        extras,
        timestamp
    ) VALUES (
        ?,?,?,?,
        ?,?,?,
        ?,?,?,
        ?,?,
        ?,?,
        ?,?,?,?,
        ?,?,?,?,?,?,?,
        ?,?,
        ?,?,
        ?,?,
        ?,?,?,?,
        ?,?,?,
        ?,?,?,?,?,
        ?,?,?,
        ?,?,?,
        ?,
        COALESCE(?, datetime('now'))
    )";

    conn.execute(
        sql,
        params![
            record.workspace_id,
            record.session_id,
            record.repo_id,
            date,
            record.provider,
            record.model,
            record.tool,
            record.cost_usd.unwrap_or(0.0),
            record.input_tokens.unwrap_or(0),
            record.output_tokens.unwrap_or(0),
            record.cache_read_tokens.unwrap_or(0),
            record.cache_write_tokens.unwrap_or(0),
            tools_json,
            mcp_json,
            record.duration_secs.unwrap_or(0),
            commits_json,
            i64::from(record.is_pr_create.unwrap_or(false)),
            record.note,
            record.git_branch,
            record.ai_title,
            record.client_version,
            record.entrypoint,
            record.user_type,
            record.slug,
            record.inference_geo,
            record.cache_5m_tokens.unwrap_or(0),
            record.cache_1h_tokens.unwrap_or(0),
            record.web_search_requests.unwrap_or(0),
            record.web_fetch_requests.unwrap_or(0),
            record.service_tier,
            record.speed,
            record.turn_count.unwrap_or(0),
            record.tool_call_count.unwrap_or(0),
            record.sidechain_turn_count.unwrap_or(0),
            record.subagent_count.unwrap_or(0),
            record.iteration_count.unwrap_or(0),
            record.error_count.unwrap_or(0),
            record.interrupted_tool_count.unwrap_or(0),
            record.permission_mode,
            stop_reasons_json,
            hooks_json,
            skills_json,
            i64::from(record.plan_mode_used.unwrap_or(false)),
            record.approval_policy,
            record.sandbox_mode,
            record.network_access,
            i64::from(record.instructions_present.unwrap_or(false)),
            record.reasoning_count.unwrap_or(0),
            record.escalated_permission_count.unwrap_or(0),
            extras_json,
            override_ts,
        ],
    )
    .context("Failed to insert ai_session row")?;
    Ok(conn.last_insert_rowid())
}

const SELECT_COLS: &str = "id, workspace_id, session_id, repo_id, date,
    provider, model, tool,
    cost_usd, input_tokens, output_tokens,
    cache_read_tokens, cache_write_tokens,
    tools_used, mcp_servers,
    duration_secs, commits, is_pr_create, note, timestamp,
    git_branch, ai_title, client_version, entrypoint, user_type, slug, inference_geo,
    cache_5m_tokens, cache_1h_tokens,
    web_search_requests, web_fetch_requests,
    service_tier, speed,
    turn_count, tool_call_count, sidechain_turn_count, subagent_count,
    iteration_count, error_count, interrupted_tool_count,
    permission_mode, stop_reasons, hook_executions, skills_used, plan_mode_used,
    approval_policy, sandbox_mode, network_access,
    instructions_present, reasoning_count, escalated_permission_count,
    extras";

pub fn list_for_workspace(workspace_id: &str, limit: i64) -> Result<Vec<AiSession>> {
    let conn = db::read_conn()?;
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {SELECT_COLS} FROM ai_sessions WHERE workspace_id = ?1 ORDER BY id DESC LIMIT ?2"
        ))
        .context("Failed to prepare ai_sessions list query")?;
    let rows = stmt
        .query_map(params![workspace_id, limit], row_to_session)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn list_recent(limit: i64) -> Result<Vec<AiSession>> {
    let conn = db::read_conn()?;
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {SELECT_COLS} FROM ai_sessions ORDER BY id DESC LIMIT ?1"
        ))
        .context("Failed to prepare ai_sessions recent query")?;
    let rows = stmt
        .query_map(params![limit], row_to_session)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Truncate every row in `ai_sessions`. Returns the number of rows
/// deleted. Pairs with `transcripts::reset_processed_state` to fully
/// reset the cost ledger from the GUI.
pub fn delete_all() -> Result<i64> {
    let conn = db::write_conn()?;
    let affected = conn
        .execute("DELETE FROM ai_sessions", [])
        .context("Failed to truncate ai_sessions")?;
    Ok(affected as i64)
}

/// Sum of cost for the most recent PR-create session in a workspace.
/// Returns 0 when no PR-create turn has happened yet. Used by the
/// Inspector to surface "Last PR cost: $X.XX" on the commit/PR header.
pub fn last_pr_cost_for_workspace(workspace_id: &str) -> Result<f64> {
    let conn = db::read_conn()?;
    let cost: Option<f64> = conn
        .query_row(
            "SELECT COALESCE(SUM(cost_usd), 0)
             FROM ai_sessions
             WHERE workspace_id = ?1 AND is_pr_create = 1
               AND session_id = (
                 SELECT session_id FROM ai_sessions
                 WHERE workspace_id = ?1 AND is_pr_create = 1
                   AND session_id IS NOT NULL
                 ORDER BY id DESC LIMIT 1
               )",
            params![workspace_id],
            |row| row.get(0),
        )
        .ok();
    Ok(cost.unwrap_or(0.0))
}

pub fn stats_overall() -> Result<AiSessionStats> {
    let conn = db::read_conn()?;
    let mut stmt = conn.prepare(
        "SELECT
            COUNT(*),
            COALESCE(SUM(cost_usd), 0),
            COALESCE(SUM(input_tokens), 0),
            COALESCE(SUM(output_tokens), 0),
            COALESCE(SUM(cache_read_tokens), 0),
            COALESCE(SUM(cache_write_tokens), 0),
            COALESCE(SUM(duration_secs), 0)
         FROM ai_sessions",
    )?;
    let (turns, cost, in_tok, out_tok, cache_r, cache_w, dur) = stmt.query_row([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, f64>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, i64>(5)?,
            row.get::<_, i64>(6)?,
        ))
    })?;

    let mut by_provider = conn.prepare(
        "SELECT COALESCE(provider, 'unknown'), COALESCE(SUM(cost_usd), 0), COUNT(*)
         FROM ai_sessions
         GROUP BY COALESCE(provider, 'unknown')
         ORDER BY 2 DESC",
    )?;
    let providers = by_provider
        .query_map([], |row| {
            Ok(ProviderCost {
                provider: row.get::<_, String>(0)?,
                cost_usd: row.get::<_, f64>(1)?,
                turns: row.get::<_, i64>(2)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(AiSessionStats {
        total_turns: turns,
        total_cost_usd: cost,
        total_input_tokens: in_tok,
        total_output_tokens: out_tok,
        total_cache_read_tokens: cache_r,
        total_cache_write_tokens: cache_w,
        total_duration_secs: dur,
        cost_by_provider: providers,
    })
}

fn row_to_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<AiSession> {
    Ok(AiSession {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        session_id: row.get(2)?,
        repo_id: row.get(3)?,
        date: row.get(4)?,
        provider: row.get(5)?,
        model: row.get(6)?,
        tool: row.get(7)?,
        cost_usd: row.get(8)?,
        input_tokens: row.get(9)?,
        output_tokens: row.get(10)?,
        cache_read_tokens: row.get(11)?,
        cache_write_tokens: row.get(12)?,
        tools_used: row.get(13)?,
        mcp_servers: row.get(14)?,
        duration_secs: row.get(15)?,
        commits: row.get(16)?,
        is_pr_create: row.get::<_, i64>(17)? != 0,
        note: row.get(18)?,
        timestamp: row.get(19)?,
        git_branch: row.get(20)?,
        ai_title: row.get(21)?,
        client_version: row.get(22)?,
        entrypoint: row.get(23)?,
        user_type: row.get(24)?,
        slug: row.get(25)?,
        inference_geo: row.get(26)?,
        cache_5m_tokens: row.get(27)?,
        cache_1h_tokens: row.get(28)?,
        web_search_requests: row.get(29)?,
        web_fetch_requests: row.get(30)?,
        service_tier: row.get(31)?,
        speed: row.get(32)?,
        turn_count: row.get(33)?,
        tool_call_count: row.get(34)?,
        sidechain_turn_count: row.get(35)?,
        subagent_count: row.get(36)?,
        iteration_count: row.get(37)?,
        error_count: row.get(38)?,
        interrupted_tool_count: row.get(39)?,
        permission_mode: row.get(40)?,
        stop_reasons: row.get(41)?,
        hook_executions: row.get(42)?,
        skills_used: row.get(43)?,
        plan_mode_used: row.get::<_, i64>(44)? != 0,
        approval_policy: row.get(45)?,
        sandbox_mode: row.get(46)?,
        network_access: row.get(47)?,
        instructions_present: row.get::<_, i64>(48)? != 0,
        reasoning_count: row.get(49)?,
        escalated_permission_count: row.get(50)?,
        extras: row.get(51)?,
    })
}
