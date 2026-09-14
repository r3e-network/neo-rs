@echo off
REM ========================================
REM Neo-N3 Performance Benchmark Runner
REM Automated 24-hour benchmark execution
REM ========================================

echo ========================================
echo Neo-N3 Performance Benchmark Suite
echo ========================================
echo.

REM Configuration
SET BENCHMARK_DURATION=86400
SET SAMPLE_INTERVAL=10

echo [Configuration]
echo Benchmark Duration: %BENCHMARK_DURATION% seconds (%BENCHMARK_DURATION%/3600 hours)
echo Sample Interval: %SAMPLE_INTERVAL% seconds
echo.

REM Check if Python is available
where python >nul 2>&1
if %errorlevel% neq 0 (
    echo [ERROR] Python not found in PATH
    echo Please install Python 3.8+ and add to PATH
    goto :install_deps
)

echo [Step 1/3] Installing dependencies...
python -m pip install --quiet requests prometheus-client psutil
if %errorlevel% neq 0 (
    echo [ERROR] Failed to install dependencies
    goto :install_deps
)
echo Dependencies installed successfully!
echo.

REM Create output directory
mkdir logs\benchmark 2>nul

echo [Step 2/3] Starting metrics collection...
echo This will run for %BENCHMARK_DURATION% seconds (%BENCHMARK_DURATION/3600 hours)%
echo Press Ctrl+C to stop early (results will be saved)
echo.

REM Run the collector
python scripts\benchmark\collect_metrics.py --duration %BENCHMARK_DURATION% --interval %SAMPLE_INTERVAL%
if %errorlevel% neq 0 (
    echo [ERROR] Metrics collection failed
    goto :done
)

echo.
echo [Step 3/3] Analyzing results...
echo.

REM Run analysis
python scripts\benchmark\analyze_results.py
if %errorlevel% neq 0 (
    echo [WARNING] Analysis completed with warnings
)

echo.
echo ========================================
echo Benchmark Complete!
echo ========================================
echo Results saved to:
echo   - logs/benchmark/metrics_history.json (raw data)
echo   - logs/benchmark/benchmark_summary.json (summary)
echo.
echo View text report above for key metrics
echo ========================================

goto :done

:install_deps
echo.
echo To install dependencies, run:
echo   pip install requests prometheus-client psutil
echo.
exit /b 1

:done
pause
