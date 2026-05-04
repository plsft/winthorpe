#!/usr/bin/env pwsh
# Launch Winthorpe in dev mode on Windows with the MSVC env already hydrated.
#
# `bun run dev` -> `tauri dev` -> cargo build needs cl.exe / link.exe / LIB /
# INCLUDE all set up. Without that you hit "link.exe failed: missing operand"
# or "cannot open input file 'kernel32.lib'" depending on Git Bash PATH.
#
# Use this from a regular PowerShell prompt:
#   pwsh -ExecutionPolicy Bypass -File scripts\dev-win.ps1
# or via the package.json script:
#   bun run dev:win

$ErrorActionPreference = 'Stop'

# Resolve bun's location *before* we touch the env, so we can re-pin it after
# vcvars hydration. We can't trust the post-vcvars PATH or the merge logic
# alone — Tauri spawns the beforeDevCommand through cmd.exe, and any drop of
# bun's dir from PATH at any link in the chain takes the whole build down.
$bunCmd = Get-Command bun -ErrorAction SilentlyContinue
if (-not $bunCmd) {
    throw 'bun not found on PATH. Install Bun: https://bun.sh'
}
$bunDir = Split-Path -Parent $bunCmd.Source

# Find newest installed Visual Studio (2026 > 2022).
$vsCandidates = @(
    'C:\Program Files\Microsoft Visual Studio\2026\VC\Auxiliary\Build\vcvars64.bat',
    'C:\Program Files\Microsoft Visual Studio\2026\Professional\VC\Auxiliary\Build\vcvars64.bat',
    'C:\Program Files\Microsoft Visual Studio\2026\Enterprise\VC\Auxiliary\Build\vcvars64.bat',
    'C:\Program Files\Microsoft Visual Studio\2026\Community\VC\Auxiliary\Build\vcvars64.bat',
    'C:\Program Files\Microsoft Visual Studio\2022\Professional\VC\Auxiliary\Build\vcvars64.bat',
    'C:\Program Files\Microsoft Visual Studio\2022\Enterprise\VC\Auxiliary\Build\vcvars64.bat',
    'C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat'
)
$vcvars = $vsCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $vcvars) {
    throw 'No Visual Studio installation with vcvars64.bat found. Install VS 2026 or 2022 with the C++ workload.'
}
Write-Host "Hydrating MSVC env from: $vcvars" -ForegroundColor Cyan

# Snapshot PATH before hydration so we can re-merge any entries vcvars drops.
# Some VS installs emit a curated PATH from `set` that omits System32 (and
# therefore `where.exe`, `cmd` builtins, etc.) — that breaks any child tool
# that shells out via `execSync("where ...")` or similar.
$originalPath = $env:Path

# Capture env after vcvars and apply to current session.
$envBlock = & cmd /c "`"$vcvars`" && set"
foreach ($line in $envBlock) {
    if ($line -match '^([^=]+)=(.*)$') {
        Set-Item -Path "Env:$($Matches[1])" -Value $Matches[2]
    }
}

# Re-merge any pre-hydration PATH entries that vcvars dropped, appended so the
# VS toolchain dirs still win on lookups.
$newParts = @($env:Path -split ';' | Where-Object { $_ })
$missing = @($originalPath -split ';' | Where-Object { $_ -and ($newParts -notcontains $_) })
if ($missing.Count -gt 0) {
    $env:Path = "$env:Path;$($missing -join ';')"
}

# Add vswhere to PATH (used by tauri-build to locate VS).
$env:Path = "C:\Program Files (x86)\Microsoft Visual Studio\Installer;$env:Path"

# Pin bun's directory on PATH explicitly. tauri spawns the beforeDevCommand
# (`bun x vite`) through cmd.exe; if bun isn't reachable there we get
# "'bun' is not recognized as an internal or external command".
if (($env:Path -split ';') -notcontains $bunDir) {
    $env:Path = "$bunDir;$env:Path"
}

# Tauri's beforeDevCommand spawn on Windows has been observed to lose PATH
# entries from the parent shell, so PATH-pinning alone isn't enough. Export
# bun's absolute path via env so scripts/dev-vite.cmd (the Windows-only
# beforeDevCommand wrapper) can invoke bun without any PATH lookup.
$env:WINTHORPE_BUN = $bunCmd.Source

# Disable sccache (project default rustc-wrapper assumes it's installed).
$env:CARGO_BUILD_RUSTC_WRAPPER = ''

Set-Location (Join-Path $PSScriptRoot '..')
Write-Host "bun: $($bunCmd.Source)" -ForegroundColor DarkGray
Write-Host "Running 'bun run dev' under hydrated MSVC env..." -ForegroundColor Cyan
& bun run dev
