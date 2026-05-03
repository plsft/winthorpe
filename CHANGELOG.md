# winthorpe

## 0.6.0

### Minor Changes

- Comprehensive AI session tracking across Claude Code, Codex, and GitHub Copilot CLI:
  - Add GitHub Copilot CLI as a tracked provider — sessions under `~/.copilot/session-state` now show up in the cost dashboard alongside Claude and Codex.
  - Capture far more per-session detail from disk transcripts: git branch, Claude-generated session title, client version, permission mode, hook execution stats, skill invocations, plan-mode usage, stop-reason mix, sidechain/subagent counts, server-side web_search / web_fetch calls, and (for Codex) sandbox mode, approval policy, network access, and escalated-permission shells.
  - Add a new prompt-history view that ingests every prompt you've typed across all three CLIs — pulled from `~/.claude/history.jsonl`, `~/.codex/history.jsonl`, `~/.copilot/command-history-state.json`, and per-session transcripts — so you can search or pivot prompts even for sessions launched outside Winthorpe.
  - Fix Anthropic 1-hour cache pricing — previously billed at the 5-minute rate, now correctly billed at 2× input. Existing rows recompute on the next scan.
  - Bill Anthropic web_search server-tool calls at $10 per 1,000 requests so cost totals match the invoice.
  - Recover Codex sessions from the older transcript format that pre-dates `token_count` events — they were silently dropped before; now appear with sandbox posture, tool counts, and turn counts.
  - Workspace attribution now prefers `gitBranch` over the cwd directory-name heuristic, so sessions land on the right workspace even when cwd is ambiguous.
