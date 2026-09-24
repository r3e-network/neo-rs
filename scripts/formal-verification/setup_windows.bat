@echo off
REM Formal Verification Setup Script for Windows
REM Installs Coq 8.19.2 + TLA+ Toolbox via manual steps

echo ============================================================
echo  Neo-N3 Formal Verification Environment Setup - Windows
echo ============================================================
echo.

echo This script guides you through installing Coq and related tools.
echo Due to Windows limitations, we'll use Chocolatey package manager.
echo.
echo Prerequisites:
echo   - Install Chocolatey: https://chocolatey.org/install
echo   - Run PowerShell as Administrator first
echo.

pause

echo ================================================
echo Step 1: Check if OPAM/OCaml is installed
echo ================================================
where opam >nul 2>&1
if %errorlevel% equ 0 (
    echo OPAM found! Version:
    opam --version
) else (
    echo OPAM not found. Install via Chocolatey:
    echo   choco install ocaml
)

echo.
echo ================================================
echo Step 2: Install Coq via Chocolatey or Docker
echo ================================================
echo Option A - Chocolatey (native):
echo   choco install coq
echo.
echo Option B - Docker (recommended for consistency):
echo   docker pull hzp/docker-coq
echo.
echo For now, we'll assume you'll install manually or skip this step.
echo You can proceed with TLA+ model checking separately.

echo.
echo ================================================
echo Step 3: Verify Formal Directory Structure
echo ================================================
if exist formal\consensus\model\node.tla (
    echo TLA+ specification found: formals\consensus\model\node.tla
) else (
    echo ERROR: node.tla not found!
    exit /b 1
)

if exist formal\coqlib\example_proof.v (
    echo Example Coq proof found: formals\coqlib\example_proof.v
) else (
    echo ERROR: example_proof.v not found!
    exit /b 1
)

if exist formal\Makefile (
    echo Makefile automation found: formal\Makefile
) else (
    echo ERROR: Makefile not found!
    exit /b 1
)

echo.
echo ================================================
echo SETUP COMPLETE!
echo ================================================
echo.
echo Next steps:
echo 1. Install Coq 8.19.2 via Chocolatey OR Docker
echo 2. Navigate to formal directory: cd formal
echo 3. Run: make coq-quick (to test compilation)
echo 4. Install TLA+ Toolbox from tlaplus.org and run TLC
echo.
echo Full instructions: docs/formal-verification/QUICKSTART.md
echo.
pause
