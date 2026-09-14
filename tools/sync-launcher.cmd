@echo off
REM Neo-N3 Testnet Sync - Main Launcher (Windows)
REM Provides unified interface for all sync operations

setlocal EnableDelayedExpansion

set SCRIPT_DIR=%~dp0
set PROJECT_ROOT=%SCRIPT_DIR%..\\..

cd /d "%PROJECT_ROOT%"

echo ===================================================================
echo     Neo-N3 Testnet Full Sync Toolkit                      
echo     Real-time Monitoring & State Validation                   
echo ===================================================================
echo.

REM Check if help requested
if /i "%1"=="help" goto show_help
if /i "%1"=="--help" goto show_help
if /i "%1"=="-h" goto show_help

REM Process command
set COMMAND=%1
shift
set ARGUMENTS=%*

if "%COMMAND%"=="" goto show_help

if /i "%COMMAND%"=="sync" (
    echo Starting full synchronization...
    if "%~1" NEQ "" (
        echo Resuming from block #%1
        set RESUME_ARG=%1
    ) else (
        set RESUME_ARG=0
    )
    echo.
    
    REM Verify neo-node exists
    if not exist "target\release\neo-node.exe" (
        echo [ERROR] Binary not found. Run: cargo build --release
        exit /b 1
    )
    
    REM Execute main sync script
    call tools\sync-monitor\sync.bat sync %RESUME_ARG%
    goto end
)

if /i "%COMMAND%"=="monitor" (
    echo Starting monitoring service...
    python3 tools\sync-monitor\sync_monitor.py --config config\testnet-production.toml
    goto end
)

if /i "%COMMAND%"=="test" (
    echo Running simulation mode...
    echo (No real nodes will be started)
    echo.
    python3 tools\sync-monitor\sync_monitor.py --dry-run
    goto end
)

if /i "%COMMAND%"=="verify" (
    echo Verifying toolkit installation...
    python3 tools\sync-monitor\verify_installation.py
    goto end
)

:show_help
echo ===================================================================
echo Usage: sync-launcher.cmd [COMMAND] [OPTIONS]
echo ===================================================================
echo.
echo Commands:
echo   sync [BLOCK]      Start full synchronization
echo                     Optional BLOCK parameter to resume from height
echo   
echo   monitor           Start monitoring dashboard only
echo   test              Run simulation without actual node
echo   verify            Check toolkit installation
echo   
echo   help              Show this help message
echo.
echo Examples:
echo   sync-launcher.cmd sync               ^# Full sync from genesis
echo   sync-launcher.cmd sync 5000000       ^# Resume from block 5M
echo   sync-launcher.cmd test               ^# Simulation mode
echo   sync-launcher.cmd verify             ^# Verify setup
echo.
echo Additional Info:
echo   Dashboard: http://localhost:8080
echo   Logs: logs\neo-sync-*.log
echo   Reports: reports\final-sync-report-*.md
echo.
echo Documentation:
echo   See README.md or QUICK_START.md for detailed usage
echo ===================================================================
goto end

:end
exit /b 0
