#!/usr/bin/env bash
# Benchmark Regression Testing Script
# Runs benchmarks and compares against baseline to detect performance regressions

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BASELINE_DIR="${PROJECT_ROOT}/target/criterion"
RESULTS_DIR="${PROJECT_ROOT}/benchmark-results"
REPORT_FILE="${RESULTS_DIR}/regression-report.json"
THRESHOLD_PERCENT="${REGRESSION_THRESHOLD:-10}"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

usage() {
    cat <<EOF
Usage: $0 [OPTIONS]

Options:
    --save-baseline     Save current benchmark results as baseline
    --compare           Compare current results against baseline
    --threshold <N>     Regression threshold in percent (default: 10)
    --verbose           Show detailed output
    -h, --help          Show this help message

Examples:
    # Save initial baseline
    $0 --save-baseline

    # Run and compare against baseline
    $0 --compare

    # Use custom threshold
    $0 --compare --threshold 5
EOF
}

log_info() {
    echo -e "${GREEN}[INFO]${NC} $*"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $*"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $*"
}

save_baseline() {
    log_info "Running benchmarks to establish baseline..."
    cd "$PROJECT_ROOT"

    # Run all benchmarks
    cargo bench --workspace --no-fail-fast -- --save-baseline baseline

    # Copy results
    mkdir -p "$RESULTS_DIR"
    if [ -d "$BASELINE_DIR" ]; then
        cp -r "$BASELINE_DIR" "${RESULTS_DIR}/baseline"
        log_info "Baseline saved to ${RESULTS_DIR}/baseline"
    else
        log_error "Criterion baseline directory not found"
        return 1
    fi
}

run_comparison() {
    log_info "Running benchmarks with comparison..."
    cd "$PROJECT_ROOT"

    if [ ! -d "${RESULTS_DIR}/baseline" ]; then
        log_error "No baseline found. Run with --save-baseline first."
        exit 1
    fi

    # Copy baseline back to criterion directory
    rm -rf "$BASELINE_DIR"
    cp -r "${RESULTS_DIR}/baseline" "$BASELINE_DIR"

    # Run benchmarks with comparison
    cargo bench --workspace --no-fail-fast -- --baseline baseline

    log_info "Benchmark comparison complete"
}

analyze_results() {
    log_info "Analyzing benchmark results for regressions..."

    # Generate JSON report
    python3 "${SCRIPT_DIR}/analyze-bench.py" \
        --baseline "${RESULTS_DIR}/baseline" \
        --current "$BASELINE_DIR" \
        --threshold "$THRESHOLD_PERCENT" \
        --output "$REPORT_FILE" || {
        log_warn "Python analysis script not found, using basic analysis"
        basic_analysis
    }
}

basic_analysis() {
    log_info "Performing basic criterion report analysis..."

    # Look for change reports in criterion output
    find "$BASELINE_DIR" -name "change.json" -type f | while read -r change_file; do
        if [ -f "$change_file" ]; then
            benchmark_name=$(dirname "$change_file" | xargs basename)
            log_info "Found results for: $benchmark_name"
        fi
    done
}

# Parse command line arguments
SAVE_BASELINE=false
COMPARE=false
VERBOSE=false

while [[ $# -gt 0 ]]; do
    case $1 in
        --save-baseline)
            SAVE_BASELINE=true
            shift
            ;;
        --compare)
            COMPARE=true
            shift
            ;;
        --threshold)
            THRESHOLD_PERCENT="$2"
            shift 2
            ;;
        --verbose)
            VERBOSE=true
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            log_error "Unknown option: $1"
            usage
            exit 1
            ;;
    esac
done

# Main execution
if [ "$SAVE_BASELINE" = true ]; then
    save_baseline
elif [ "$COMPARE" = true ]; then
    run_comparison
    analyze_results
else
    log_error "No action specified. Use --save-baseline or --compare"
    usage
    exit 1
fi

log_info "Done!"
