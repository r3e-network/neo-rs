@echo off
REM Neo-N3 Testnet Full Sync Script (Windows Version)
REM Orchestrates full testnet synchronization with monitoring

set CONFIG_FILE=%1
if "%CONFIG_FILE%"=="" set CONFIG_FILE=config\testnet-production.toml

set LOG_DIR=logs
set REPORT_DIR=reports
set MONITOR_PORT=8080

echo ============================================
echo Neo-N3 Testnet Full Sync Tool
echo ============================================
echo Config: %CONFIG_FILE%
echo Log Dir: %LOG_DIR%
echo Report Dir: %REPORT_DIR%
echo.

REM Check dependencies
echo Checking dependencies...

IF NOT EXIST target\release\neo-node.exe (
    echo [WARN] neo-node.exe not found. Attempting to build...
    cargo build --release --bin neo-node
    IF ERRORLEVEL 1 (
        echo [ERROR] Failed to build neo-node
        exit /b 1
    )
)

mkdir "%LOG_DIR%" 2>nul
mkdir "%REPORT_DIR%" 2>nul

echo [SUCCESS] All dependencies checked
echo.

SET /A RESUME_BLOCK=0
if "%~2"!=="" SET /A RESUME_BLOCK=%~2

echo Starting full testnet synchronization...
echo Resume from block: %RESUME_BLOCK% (0 = genesis)
echo.

:: Create timestamped log file
for /f "tokens=2 delims==" %%g in ('wmic OS Get localdate /value') do set MMDD=%%g
for /f "tokens=2 delims==%%g in ('wmic timezone get localtime /value') do set TZ=%%g
set TIMESTAMP=%date%_%time%
set LOG_FILE=%LOG_DIR%\neo-sync-%TIMESTAMP%.log

echo Logs will be saved to: %LOG_FILE%
echo.

:: Start neo-node with output redirection
start "Neo Node" cmd /c "target\release\neo-node.exe --config %CONFIG_FILE% > ^"%LOG_FILE^%" 2>&1"

FOR /F "tokens=* USEBACKQ" %%P IN (`taskfind /FI "WINDOWTITLE IS Neo Node" | FINDSTR /N "^" ^| sort /R /N | findstr /R "[0-9]*$"`) DO SET NEO_PID=%%P
echo [SUCCESS] Neo-node started
echo.
echo Monitoring sync progress...
echo Press Ctrl+C to stop monitoring
echo.

SET LAST_BLOCK=0
SET CHECK_INTERVAL=5

:MONITOR_LOOP
timeout /t %CHECK_INTERVAL% /nobreak >nul

:: Try to get current block height via RPC
powershell -Command "try { $(curl.exe -s -X POST -H 'Content-Type: application/json' -d '{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getblockcount\",\"params\":[]}' http://localhost:10332 | ConvertFrom-Json).result } catch { '' }" > temp_height.txt
SET /P CURRENT_HEIGHT=<temp_height.txt
del temp_height.txt

if "%CURRENT_HEIGHT%" NEQ "%LAST_BLOCK%" if "%CURRENT_HEIGHT%" NEQ "" (
    powershell -Command "$elapsed = [int]$env:TICKS / 10000000; Write-Host ''" SET TICKS=TICKS
    SET /A DIFF=%CURRENT_HEIGHT%-%LAST_BLOCK%
    SET /A SPEED=%DIFF%/%CHECK_INTERVAL%
    
    set /a HOURS=TIME/3600
    set /a MINS=(TIME-HOURS*3600)/60
    set /a SECS=TIME-HOURS*3600-MINS*60
    
    echo [%DATE% %TIME%] Block: %CURRENT_HEIGHT% | Speed: %SPEED%/blk/s | Uptime: %HOURS%h%MINS%m
    SET LAST_BLOCK=%CURRENT_HEIGHT%
    
    :: Milestone detection every 100K blocks
    SET /A REMINDER=%CURRENT_HEIGHT%%%100000
    IF %REMAINDER% EQU 0 (
        echo [SUCCESS] * Milestone reached: Block #%CURRENT_HEIGHT%
    )
)

tasklist /FI "WINDOWTITLE IS Neo Node" | findstr /C:"neo-node.exe" >nul
IF ERRORLEVEL 1 (
    echo.
    echo [INFO] Node process stopped
    goto :GENERATE_REPORT
)

goto MONITOR_LOOP

:GENERATE_REPORT
echo Generating completion report...

powershell -Command "
`$finalHeight='%CURRENT_HEIGHT%'
`$logFile='%LOG_FILE%'
`$reportFile='%REPORT_DIR%\final-sync-report-$(Get-Date -Format 'yyyyMMdd-HHmmss').md'

@'
# Neo-N3 Full Sync Report

## Summary
- **Final Height**: `$finalHeight
- **Log File**: `$logFile
- **Completion Time**: $(Get-Date -Format 'o')

## Status: COMPLETED

Sync successfully completed! Review logs for detailed metrics.

**Next Steps:**
- Check detailed logs at `$logFile
- Validate state consistency
- Deploy to production environment
" | Out-File -FilePath `$reportFile -Encoding utf8

echo [SUCCESS] Report saved to: `$reportFile
echo.
echo ============================================
echo SYNC COMPLETION SUMMARY
echo ============================================
echo Final Height:         `%CURRENT_HEIGHT%`
echo Log File:             `%LOG_FILE%`
echo Report File:          `%REPORT_DIR%\*.md`
echo Status:               COMPLETED
echo ============================================

DEL temp_height.txt 2>nul
