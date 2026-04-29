# Winthorpe — architecture notes

Living document for cross-cutting decisions and platform-specific gotchas.
Updated as features land.

## Stack snapshot

- **Frontend:** Tauri 2 + React 19 + Vite + TypeScript + Tailwind 4
- **Backend:** Rust (`src-tauri/`) — SQLite via rusqlite (bundled), notify
  for FS watching, portable-pty for ConPTY/PTY abstraction, windows-rs for
  Job Objects + DPAPI + Registry
- **Sidecar runtimes:**
  - Bun (`sidecar/`) — primary LLM session host (Claude Agent SDK + Codex SDK)
  - .NET 10 AOT (`sidecar-dotnet/`) — sub-host for C# user skills
- **Bundle format:** NSIS + MSI on Windows; DMG/.app on macOS

## Platform-specific decisions

| Subsystem | Implementation |
|---|---|
| PTY | `portable-pty 0.8` (ConPTY-backed on Windows, POSIX PTY on Unix) |
| Process supervision | Win32 Job Objects (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`) on Windows; `setpgid` + `killpg` on Unix |
| Credentials | SQLite + DPAPI (per-user) on Windows; macOS Keychain on macOS |
| Editor detection | Registry App Paths walk + standard install dirs on Windows; Spotlight `mdfind` on macOS |
| Reveal in OS file manager | `explorer.exe /select,<path>` on Windows; `open -R <path>` on macOS |
| CLI install | `.cmd` shim in `%LOCALAPPDATA%\Programs\Winthorpe\bin\` + user-PATH update + `WM_SETTINGCHANGE` broadcast on Windows; symlink to `/usr/local/bin` via `osascript` UAC on macOS |
| Deep links | `tauri-plugin-deep-link` + registry under `HKCU\Software\Classes\winthorpe` on Windows; LaunchServices on macOS |
| Title bar | Custom in-app chrome on every platform (`decorations: false`); Mica backdrop on Windows 11 |
| Quit confirmation | Window-close + system-shutdown handler |

## Open follow-ups

- Authenticode signing for Windows release artifacts
- Brand artwork refresh — current logo is the geometric W tile pattern
  (auto-generated). Replace with hand-designed marks when available.
- Filesystem watcher payload-size cap is currently 200 paths — bump if any
  workflow exceeds it.
