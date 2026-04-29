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

# Capture env after vcvars and apply to current session.
$envBlock = & cmd /c "`"$vcvars`" && set"
foreach ($line in $envBlock) {
    if ($line -match '^([^=]+)=(.*)$') {
        Set-Item -Path "Env:$($Matches[1])" -Value $Matches[2]
    }
}

# Add vswhere to PATH (used by tauri-build to locate VS).
$env:Path = "C:\Program Files (x86)\Microsoft Visual Studio\Installer;$env:Path"

# Disable sccache (project default rustc-wrapper assumes it's installed).
$env:CARGO_BUILD_RUSTC_WRAPPER = ''

Set-Location (Join-Path $PSScriptRoot '..')
Write-Host "Running 'bun run dev' under hydrated MSVC env..." -ForegroundColor Cyan
& bun run dev
