@echo off
REM Tauri's beforeDevCommand on Windows spawns through cmd.exe with a sanitized
REM PATH that drops bun's install dir (and often System32). Invoking `bun x vite`
REM directly therefore fails with "'bun' is not recognized". We sidestep PATH
REM by launching bun via its absolute path: WINTHORPE_BUN if set by
REM scripts/dev-win.ps1, otherwise the standard install location.
setlocal
if defined WINTHORPE_BUN (
    set "BUN_EXE=%WINTHORPE_BUN%"
) else (
    set "BUN_EXE=%USERPROFILE%\.bun\bin\bun.exe"
)
if not exist "%BUN_EXE%" (
    echo [dev-vite] bun.exe not found at "%BUN_EXE%"
    echo            Install Bun from https://bun.sh, or set WINTHORPE_BUN to your bun.exe path.
    exit /b 1
)
"%BUN_EXE%" x vite
