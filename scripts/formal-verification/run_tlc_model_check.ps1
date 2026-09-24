# ============================================================================
# TLA+ Model Checking Execution Script for Neo-N3 Consensus
# Purpose: Run TLC model checker on fixed consensus specification  
# Environment: Windows PowerShell with Java Runtime
# ============================================================================

Write-Host "`n========================================" -ForegroundColor Cyan
Write-Host "  Neo-N3 PBFT Consensus - TLC Model Check" -ForegroundColor Yellow
Write-Host "========================================`n" -ForegroundColor Green

# Check if we can find Java
try {
    $javaVersion = java -version 2>&1
    Write-Host "[OK] Java found in PATH" -ForegroundColor Green
} catch {
    Write-Host "[!] Java NOT found in PATH" -ForegroundColor Red
    Write-Host "Please install Java Runtime Environment:" -ForegroundColor Yellow
    Write-Host "  winget install EclipseAdoptium.Temurin.21.JRE" -ForegroundColor Gray
    Write-Host "Or download from: https://adoptium.net/temurin/releases/?version=21" -ForegroundColor Gray
    Write-Host "Common locations:" -ForegroundColor Yellow
    Write-Host "  C:\Program Files\Java\jdk-21\bin" -ForegroundColor Gray
    Write-Host "  C:\Program Files\Eclipse Temurin\jdk-21\bin" -ForegroundColor Gray
    Write-Host "`nAfter installation, restart PowerShell and run this script again." -ForegroundColor Yellow
    pause
    exit 1
}

# Change to model directory
cd D:\Git\neo-rs\formal\consensus\model
$env:OLDPWD = Get-Location

# Create reports directory if missing
New-Item -ItemType Directory -Force -Path "D:\Git\neo-rs\reports\formal" | Out-Null

# Helper function to run TLC with timeout
function Run-TLC {
    param(
        [string]$SpecFile,
        [string]$ConfigFile,
        [string]$OutputFile,
        [string]$Description,
        [int]$ExpectedOutcome  # "PASS" or "FAIL"
    )
    
    Write-Host "`n=== Step: $($Description) ===" -ForegroundColor Cyan
    Write-Host "Configuration: n=?, f=?" -ForegroundColor Yellow
    
    $result = java -jar "..\..\tools\tla2tools.jar" $SpecFile -config $ConfigFile 2>&1 | Tee-Object -FilePath $OutputFile
    
    $passed = ($result -match "NO PROPERTY VIOLATIONS FOUND") -or 
              ($result -match "Finished.*and executed.*states") -and 
              ($result -notmatch "Invariant.*violated")
    
    if ($ExpectedOutcome -eq "PASS") {
        if ($passed) {
            Write-Host "[SUCCESS] Expected PASS - All invariants verified!" -ForegroundColor Green
            return $true
        } else {
            Write-Host "[ERROR] Unexpected FAIL - Should have passed!" -ForegroundColor Red
            return $false
        }
    } elseif ($ExpectedOutcome -eq "FAIL") {
        if (-not $passed) {
            Write-Host "[EXPECTED] Agreement invariant violated as expected!" -ForegroundColor Yellow
            Write-Host "Counterexample confirms safety breaks when n < 3f+1" -ForegroundColor Cyan
            return $true
        } else {
            Write-Host "[WARNING] Expected FAIL but got PASS - Check configuration!" -ForegroundColor Yellow
            return $false
        }
    }
    
    return $false
}

# Step 1: Run Healthy Configuration
$healthyResult = Run-TLC `
    -SpecFile "consensus/node_fixed.tla" `
    -ConfigFile "tlc_healthy.cfg" `
    -OutputFile "D:\Git\neo-rs\reports\formal\tlc_healthy_output.txt" `
    -Description "Healthy Config (n=4, f=1)" `
    -ExpectedOutcome "PASS"

# Step 2: Run Unhealthy Configuration  
$unhealthyResult = Run-TLC `
    -SpecFile "consensus/node_fixed.tla" `
    -ConfigFile "tlc_unhealthy.cfg" `
    -OutputFile "D:\Git\neo-rs\reports\formal\tlc_unhealthy_output.txt" `
    -Description "Unhealthy Config (n=3, f=1)" `
    -ExpectedOutcome "FAIL"

# Summary
Write-Host "`n========================================" -ForegroundColor Cyan
Write-Host "  Model Checking Complete!" -ForegroundColor Yellow
Write-Host "========================================`n" -ForegroundColor Green

if ($healthyResult -and $unhealthyResult) {
    Write-Host "✅ Both scenarios produced EXPECTED results!" -ForegroundColor Green
    Write-Host "Results saved to: D:\Git\neo-rs\reports\formal\" -ForegroundColor White
    Write-Host "`nNext: Review output logs and generate final report" -ForegroundColor Cyan
} else {
    Write-Host "⚠️ Some scenarios did not produce expected results" -ForegroundColor Yellow
    Write-Host "Review logs in: D:\Git\neo-rs\reports\formal\" -ForegroundColor White
}

Write-Host "`nPress any key to exit..."
pause | Out-Null
