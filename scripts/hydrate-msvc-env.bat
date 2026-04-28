@echo off
REM Helper sourced from Git Bash via cmd //c.
REM Calls vcvars64.bat then prints the resulting env so the parent bash
REM can capture INCLUDE/LIB/LIBPATH and update its own environment.
REM Detects newest installed Visual Studio (2026 > 2022).

setlocal enabledelayedexpansion

set "VS_ROOT="
for %%V in (
    "C:\Program Files\Microsoft Visual Studio\2026\VC"
    "C:\Program Files\Microsoft Visual Studio\2026\Professional\VC"
    "C:\Program Files\Microsoft Visual Studio\2026\Enterprise\VC"
    "C:\Program Files\Microsoft Visual Studio\2026\Community\VC"
    "C:\Program Files\Microsoft Visual Studio\2022\Professional\VC"
    "C:\Program Files\Microsoft Visual Studio\2022\Enterprise\VC"
    "C:\Program Files\Microsoft Visual Studio\2022\Community\VC"
) do (
    if exist "%%~V\Auxiliary\Build\vcvars64.bat" (
        set "VS_ROOT=%%~V"
        goto :found
    )
)

echo error: no Visual Studio installation with vcvars64.bat found 1>&2
exit /b 1

:found
call "%VS_ROOT%\Auxiliary\Build\vcvars64.bat" >nul 2>&1
if errorlevel 1 (
    echo error: vcvars64.bat failed 1>&2
    exit /b 1
)
set
