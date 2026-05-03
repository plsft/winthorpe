# winthorpe

## 0.6.3

### Patch Changes

- Drop the orphan updater public key from `tauri.conf.json`. The pubkey was registered but the matching private key was never created, so `tauri build` kept failing with `A public key has been found, but no private key`. Without the pubkey in config, the build completes and the NSIS installer publishes. Auto-update remains off until a fresh keypair is generated.

## 0.6.2

### Patch Changes

- Disable updater-JSON publishing in the Windows release workflow until a Tauri signing keypair is set up — tauri-action's signing step was failing with `Missing comment in secret key` and blocking the release from publishing. NSIS installer ships normally; in-app auto-update will start working again once the signing secrets are populated.

## 0.6.1

### Patch Changes

- Fix release pipeline so v0.6.x actually ships:
  - Drop MSI from the Windows publish workflow — WIX `light.exe` has been failing the bundle step on every recent release. NSIS still ships as the canonical installer; enterprise SCCM/GPO consumers can still produce an MSI locally via `release-win.ps1 -IncludeMsi` once WIX is stable.
  - Fix a macOS-only compile error in `forge/cli_status.rs` — the Connect-terminal flow's call to `run_command_with_timeout` was missing its import after the Helmor → Winthorpe rename, blocking every `Quality` workflow run.
  - Stabilize the WebKit Playwright suite by raising expect/action/navigation timeouts on CI (5 s default was tripping cold starts) and waiting for the React shell to render before specs poke at it.

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
