# Winthorpe — Windows port plan

Living document tracking the macOS → Windows x64 port. Update as phases land.

## Why this fork exists

Upstream [Helmor](https://github.com/dohooo/helmor) is macOS-only by design — its NOTICE explicitly requires forks to ship under a distinct name. Winthorpe is a Windows-first fork built around the maintainer's stack: C#/.NET 10 + JS/TS as first-class peers, Bun as the default execution shell, SQLite for everything (including credentials), and Windows 11 polish (Mica, snap layouts, native Explorer integration).

Not intended for near-term public release. Optimise for correctness, longevity, and developer experience over shipping speed.

## Architectural decisions diverging from upstream

| Subsystem | Upstream (macOS) | Winthorpe (Windows) | Why |
|---|---|---|---|
| PTY | `libc::openpty` + `setsid` + `TIOCSCTTY` | `portable-pty 0.8` (ConPTY-backed) | Cross-platform; matches the maintainer's existing Tauri pattern (Kismet) |
| Process supervision | Unix process groups + `killpg` | Windows Job Objects with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` | Atomic descendant cleanup; no orphans |
| Credentials | macOS Keychain via `security` CLI + `security-framework` | SQLite table + DPAPI per-user encryption | Maintainer preference; portable + inspectable |
| Shell-env capture | Spawn `$SHELL -l -c env -0`, merge into process | No-op (Windows GUI processes inherit parent env) | Not needed on Windows |
| Editor detection | `mdfind` over Spotlight + `.app` bundle paths | Registry App Paths walk + standard install dirs (`%ProgramFiles%`, `%LOCALAPPDATA%\Programs`) | Direct Windows analog |
| Reveal in OS file manager | `open -R <path>` | `explorer.exe /select,<path>` | |
| Image to clipboard | `osascript` + AppleScript clipboard set | `arboard` crate (cross-platform) | |
| CLI install | Symlink to `/usr/local/bin` via `osascript` UAC | `.cmd` shim in `%LOCALAPPDATA%\Programs\Winthorpe\bin\` + user-PATH update + `WM_SETTINGCHANGE` broadcast | No admin needed |
| Deep links | `tauri-plugin-deep-link` + LaunchServices | Same plugin + registry under `HKCU\Software\Classes\winthorpe` | |
| Sidecar runtime | Bun (single sidecar) | Bun primary + .NET 10 AOT sub-host for C# user skills | First-class .NET per maintainer's stack |
| Bundle format | DMG + `.app.tar.gz` | NSIS installer + MSI | NSIS gives better update UX |
| Title bar | Overlay + traffic lights at (12, 20) | Overlay + Mica backdrop + custom min/max/close controls | Win11 polish |
| Quit confirmation | Cmd+Q via custom NSMenu install | Window close + system shutdown handler | |

## Phase plan

| Phase | Goal | Status |
|---|---|---|
| 0 | Clone fork, atomic rename Helmor→Winthorpe, baseline commit | **in progress** |
| 1 | cfg-gate every macOS/Unix site so `cargo check --target x86_64-pc-windows-msvc` passes | pending |
| 2 | Cross-platform PTY (portable-pty) + Job Objects for sidecar/git/forge | pending |
| 3 | Vendor staging Windows branch (codex-windows-x64, ripgrep x64-win32, gh/glab Windows zips) | pending |
| 4 | SQLite + DPAPI credential store | pending |
| 5 | Editor detection + reveal in Explorer + Windows shell ergonomics | pending |
| 6 | First-class .NET 10 — sidecar-dotnet AOT sub-host + .NET project recognition | pending |
| 7 | Windows UI polish — Mica, custom title bar, Ctrl-prefix shortcuts, deep links, CLI install | pending |
| 8 | CI on `windows-latest`, NSIS+MSI bundle, latest.json windows entry | pending |
| 9 | Performance baseline + tests rewrite for cross-platform | pending |

## Things that stayed identical

- Sidecar JSON-RPC stdin/stdout protocol (provider/method/params/id; events: end, error, aborted)
- Database schema (rusqlite + bundled SQLite) — no Unix paths in schema
- `agents/` and `pipeline/` modules — fully platform-neutral
- Tauri 2 plugin set — deep-link, dialog, notification, opener, updater, global-shortcut, mcp-bridge
- React frontend architecture (TanStack Query, Lexical, Monaco, streamdown)
- Test stack — vitest + insta + bun test, three-target test runner

## Trademark / attribution

Per upstream Helmor's NOTICE Section 4(b), modified source files carry a change notice in their header where modifications are non-trivial (rewrites, not whitespace renames). The blanket Helmor→Winthorpe rename in Phase 0 does not warrant per-file change headers; subsequent Windows-specific rewrites (PTY, credential store, etc.) should add them.
