@echo off
REM Formal Verification Installation Script for Windows - Complete Guide
REM This script provides step-by-step instructions for installing all required tools

echo ============================================================
echo  Neo-N3 Formal Verification - Complete Installation Guide
echo ============================================================
echo.
echo This guide will help you install:
echo   1. TLA+ Toolbox (for consensus model checking)
echo   2. Coq Proof Assistant (optional, for future proofs)
echo.

echo ================================================
echo Step 1: Install TLA+ Toolbox (REQUIRED)
echo ================================================
echo.
echo TLA+ Toolbox installation steps:
echo.
echo 1. Download from official website:
echo    https://www.tlaplus.org/Download/
echo.
echo 2. Click "TLA+ Toolbox v1.7.0" for Windows
echo    Download file: tla2tools-v1-7-0-windows-x86_64.exe
echo.
echo 3. Run installer as Administrator:
echo    a. Right-click downloaded .exe file
echo    b. Select "Run as administrator"
echo    c. Accept license agreement
echo    d. Use default installation directory
echo    e. Complete wizard
echo.
echo 4. Verify installation:
echo    a. Start Menu -> Search "TLA+ Toolbox"
echo    b. Launch application
echo.
echo.
pause
echo ================================================
echo Step 2: Load Neo-RS Consensus Specification
echo ================================================
echo.
echo In TLA+ Toolbox:
echo 1. File -> Import Existing Projects into Workspace
echo 2. Browse to: d:\Git\neo-rs\formal\consensus\model\
echo 3. Select folder and click OK
echo 4. Verify node.tla appears in Package Explorer
echo.
echo.
pause
echo ================================================
echo Step 3: Run Model Checking (Healthy Config)
echo ================================================
echo.
echo 1. Right-click node.tla -> Run As -> TLC Model Checker
echo 2. Click "Add" under State Invariants
echo 3. Add each invariant: Agreement, Validity, NoDoubleCommit, FaultToleranceBound
echo 4. Set Depth: 15 (default)
echo 5. Workers: 4 (adjust based on CPU cores)
echo 6. Click "Start"
echo.
echo Expected Result:
echo   ✅ NO PROPERTY VIOLATIONS FOUND
echo   All invariants VERIFIED
echo.
echo.
pause
echo ================================================
echo Step 4: Run Model Checking (Unhealthy Config)
echo ================================================
echo.
echo 1. Edit tlc.cfg file
echo 2. Change line: n = 4  TO  n = 3
echo 3. Re-run TLC as in Step 3
echo.
echo Expected Result:
echo   ❌ INVARIANT Violation: Agreement
echo   Counterexample shows two honest nodes disagreeing
echo.
echo.
pause
echo ================================================
echo Step 5: Optional - Install Coq Toolchain
echo ================================================
echo.
echo Option A: Chocolatey (Recommended if installed)
echo   choco install ocaml
echo   choco install coq
echo.
echo Option B: Manual Installer
echo   Download from: https://coq.inria.fr/download
echo   Run .exe installer
echo   Follow installation wizard
echo.
echo Option C: WSL2 (If available)
echo   wsl
echo   sudo apt-get update && sudo apt-get install -y opam ca-certificates
echo   opam init --disable-sandboxing
echo   opam switch create coq-formal 4.14.1
echo   opam install coq==8.19.2 --yes
echo   eval $(opam env)
echo.
echo.
pause
echo ================================================
echo INSTALLATION COMPLETE!
echo ================================================
echo.
echo Next Steps:
echo 1. Run TLC on healthy config (n=4,f=1) - Should pass all invariants
echo 2. Run TLC on unhealthy config (n=3,f=1) - Should violate Agreement
echo 3. Document results and screenshots
echo 4. Create verification report
echo.
echo Full documentation:
echo - docs/formal-verification/INSTALLATION-GUIDE-WINDOWS.md
echo - docs/formal-verification/TLAPLUS-RUN-GUIDE.md
echo.
pause
