@echo off
REM ============================================================================
REM  TLA+ Model Checking Execution Script for Neo-N3 Consensus
REM  Purpose: Run TLC model checker on fixed consensus specification
REM  Environment: Windows PowerShell with Java Runtime
REM ============================================================================

echo.
echo ========================================
echo   Neo-N3 PBFT Consensus - TLC Model Check
echo ========================================
echo.

REM Check if we can find Java
where java >nul 2>&1
if %errorlevel% equ 0 (
    echo [OK] Java found in PATH
    java -version
) else (
    echo [!] Java NOT found in PATH
    echo Please manually locate your JRE and set JAVA_HOME or add to PATH
    echo Common locations:
    echo   C:\Program Files\Java\jdk-21\bin
    echo   C:\Program Files\Eclipse Temurin\jdk-21\bin
    echo.
    pause
    exit /b 1
)

REM Change to model directory
cd /d "%~dp0formal\consensus\model"

echo.
echo === Step 1: Running Healthy Configuration (n=4, f=1) ===
echo Expected: NO PROPERTY VIOLATIONS FOUND
echo.

java -jar "..\..\tools\tla2tools.jar" consensus/node_fixed.tla -config tlc_healthy.cfg 2>&1 | Tee-Object -FilePath "..\..\..\reports\formal\tlc_healthy_output.txt"

if %errorlevel% equ 0 (
    echo.
    echo [SUCCESS] Healthy configuration completed
) else (
    echo.
    echo [ERROR] Healthy configuration failed
)

echo.
echo === Step 2: Running Unhealthy Configuration (n=3, f=1) ===
echo Expected: Agreement INVARIANT VIOLATION
echo.

java -jar "..\..\tools\tla2tools.jar" consensus/node_fixed.tla -config tlc_unhealthy.cfg 2>&1 | Tee-Object -FilePath "..\..\..\reports\formal\tlc_unhealthy_output.txt"

if %errorlevel% equ 0 (
    echo.
    echo [SUCCESS] Unhealthy configuration completed
) else (
    echo.
    echo [EXPECTED] Unhealthy configuration failed as expected
)

echo.
echo ========================================
echo   Both configurations executed!
echo   Results saved to: reports\formal\
echo ========================================
echo.
pause
