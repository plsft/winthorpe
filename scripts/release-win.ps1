#!/usr/bin/env pwsh
# Build a Winthorpe release (NSIS + MSI) on Windows with MSVC env hydrated.
#
# Mirrors scripts\dev-win.ps1 but invokes `tauri build` instead of `tauri dev`.
# Rust release linking requires cl.exe / link.exe / LIB / INCLUDE, which a
# plain PowerShell prompt does not have on PATH — vcvars64.bat hydrates it.
#
# Usage:
#   pwsh -ExecutionPolicy Bypass -File scripts\release-win.ps1
# or:
#   bun run release:win

$ErrorActionPreference = 'Stop'

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
    throw 'No Visual Studio installation with vcvars64.bat found. Install VS 2026 or 2022 with the "Desktop development with C++" workload.'
}
Write-Host "Hydrating MSVC env from: $vcvars" -ForegroundColor Cyan

$envBlock = & cmd /c "`"$vcvars`" && set"
foreach ($line in $envBlock) {
    if ($line -match '^([^=]+)=(.*)$') {
        Set-Item -Path "Env:$($Matches[1])" -Value $Matches[2]
    }
}

$env:Path = "C:\Program Files (x86)\Microsoft Visual Studio\Installer;$env:Path"
$env:CARGO_BUILD_RUSTC_WRAPPER = ''

Set-Location (Join-Path $PSScriptRoot '..')

# NSIS is the default-shipped installer (matches what Tauri's docs recommend
# for general distribution). MSI is opt-in via -IncludeMsi for enterprise
# scenarios — group policy, SCCM, etc. — and pulls in WIX (light.exe) which
# has its own failure modes around long paths and ICE validations.
$bundles = if ($env:WINTHORPE_RELEASE_BUNDLES) { $env:WINTHORPE_RELEASE_BUNDLES } else { 'nsis' }

# Tagged releases use the `release-fat` Cargo profile: lto = "fat" instead of
# "thin". Trades 2-3x build time for ~5-10% smaller / 3-5% faster binary.
# Override with WINTHORPE_RELEASE_PROFILE=release for an iterating dev-style
# bundled build that still uses NSIS.
$rustProfile = if ($env:WINTHORPE_RELEASE_PROFILE) { $env:WINTHORPE_RELEASE_PROFILE } else { 'release-fat' }

Write-Host "Running 'tauri build' (bundles: $bundles, profile: $rustProfile, x86_64-pc-windows-msvc) under hydrated MSVC env..." -ForegroundColor Cyan
& bun run tauri build --target x86_64-pc-windows-msvc --bundles $bundles -- --profile $rustProfile
