#!/usr/bin/env bash
# Benchmark Regression Testing Script
# Runs benchmarks and compares against baseline to detect performance regressions

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BASELINE_DIR="${PROJECT_ROOT}/target/criterion"
RESULTS_DIR="${PROJECT_ROOT}/benchmark-results"
REPORT_FILE="${RESULTS_DIR}/regression-report.json"
# Percentage threshold for a change to count as a regression.
#
# 20%, not the usual 10%: two back-to-back gate runs on the development host,
# on unchanged code, reported 10.4%, 11.9% and 16.0% "regressions" — that is
# host noise, not code. A threshold below the noise floor turns the gate into a
# coin flip, and a gate people learn to retry is worse than no gate. Real
# regressions on these paths (the perf work moved a 6,001-block range from
# 286 s to 118 s) are far larger than 20%.
THRESHOLD_PERCENT="${REGRESSION_THRESHOLD:-20}"

# Absolute minimum effect for a change to count. A percentage threshold alone
# lets a 1.4 us microbenchmark swing 16% (0.2 us) and block a merge. 10 us
# excludes that noise while still letting a 20% move in a 57 us engine-setup
# benchmark through, which is the smallest change that matters at block scale.
MIN_ABS_NS="${REGRESSION_MIN_ABS_NS:-10000}"

# Benchmarks that block a merge. Everything else still runs under
# `--save-baseline` / `--compare` and shows up in the report, but is advisory.
#
# Why a subset: absolute criterion timings are only comparable on the same
# machine under the same load. The full suite takes ~38 min per pass, so a
# same-job baseline + comparison of every target does not fit a sane CI budget.
#
# `block_persistence` is deliberately excluded. Three consecutive gate runs on
# unchanged code reported +11.9%, then an improvement, then +22.1% for
# `persist_block_one_ret_tx`; that is a noise-dominated benchmark, and gating on
# it would fail merges at random. Its numbers stay in the report as trend data.
GATE_BENCHES=(mpt_state_root native_contracts)

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

usage() {
    cat <<EOF
Usage: $0 [OPTIONS]

Options:
    --gate              Run the blocking gate: measure a fixed subset of
                        benchmarks twice on this machine (baseline, then
                        comparison) and fail on a regression
    --save-baseline     Run every criterion target and store a baseline
    --compare           Run every criterion target against a stored baseline
    --threshold <N>     Regression threshold in percent (default: 20)
    --verbose           Show detailed output
    -h, --help          Show this help message

Examples:
    # Blocking CI gate (same-machine comparison, no cross-machine baseline)
    $0 --gate

    # Record / recheck the full suite locally
    $0 --save-baseline
    $0 --compare

    # Use custom threshold
    $0 --gate --threshold 5
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

# Lists the criterion bench targets of every workspace member as
# "<package>\t<bench>" lines.
#
# `cargo bench --workspace` runs far more than the criterion suites: it also
# builds and runs each crate's lib/binary test harness, plus any `benches/*.rs`
# picked up by autodiscovery with the default libtest harness. Those harnesses
# measure nothing here (this repo has no nightly `#[bench]` functions), but
# they do parse the command line and abort with
#   error: Unrecognized option: 'save-baseline'
# which makes the whole `cargo bench` invocation exit non-zero regardless of
# `--no-fail-fast`, so the baseline was never produced.
#
# Selecting only targets declared as `[[bench]]` with `harness = false` keeps
# every criterion benchmark in the workspace (and picks up new ones
# automatically, since the manifests are parsed rather than a name list being
# hard-coded) while skipping harnesses that cannot accept criterion's flags.
#
# The package is always emitted next to the bench name: `serialization` exists
# in both `neo-core` and `neo-io`, so a bare `--bench serialization` would
# select several packages and make the "which targets ran" question unanswerable.
#
# `harness` is not exposed by `cargo metadata`, hence the manifest parse.
# Targets whose `required-features` are not satisfied by the package's default
# feature set are omitted, mirroring cargo's own skip for a plain `--workspace`
# run (the script never enables optional features).
criterion_bench_pairs() {
    python3 - "$PROJECT_ROOT/Cargo.toml" <<'PY'
import json
import subprocess
import sys

try:
    import tomllib
except ModuleNotFoundError:  # Python < 3.11
    sys.exit("bench-regression.sh: Python 3.11+ is required (tomllib)")

from pathlib import Path


def enabled_features(package):
    """Default feature set, expanded transitively (this script enables none)."""
    features = package.get("features") or {}
    enabled = set(features.get("default", []))
    pending = list(enabled)
    while pending:
        feature = pending.pop()
        for implied in features.get(feature, []):
            if implied in features and implied not in enabled:
                enabled.add(implied)
                pending.append(implied)
    return enabled


manifest = sys.argv[1]
metadata = json.loads(
    subprocess.run(
        [
            "cargo",
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
            manifest,
        ],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
)

pairs = set()
for package in metadata.get("packages", []):
    package_manifest = package.get("manifest_path")
    if not package_manifest:
        continue
    with Path(package_manifest).open("rb") as handle:
        document = tomllib.load(handle)
    enabled = enabled_features(package)
    for bench in document.get("bench", []):
        name = bench.get("name")
        if not name:
            continue
        # libtest harnesses (the default) reject criterion's CLI options.
        if bench.get("harness", True) is not False:
            continue
        if not set(bench.get("required-features") or []) <= enabled:
            continue
        pairs.add((package["name"], name))

for package_name, bench_name in sorted(pairs):
    print(f"{package_name}\t{bench_name}")
PY
}

# Populated by load_bench_pairs(): "<package>\t<bench>" entries.
BENCH_PAIRS=()

# Fills BENCH_PAIRS, failing loudly when discovery comes up empty: an empty
# selection would make the whole gate report "0 benchmarks, green".
#
# Optional arguments restrict the selection to those bench names. Every
# requested name must match at least one target, otherwise a typo in
# GATE_BENCHES would quietly shrink the gate.
load_bench_pairs() {
    local -a wanted=("$@")
    local package name want keep found
    BENCH_PAIRS=()
    while IFS=$'\t' read -r package name; do
        # Python's stdout translates "\n" to "\r\n" on Windows; strip the CR so
        # the pair matches a real cargo target there too.
        package="${package%$'\r'}"
        name="${name%$'\r'}"
        if [ -z "$package" ] || [ -z "$name" ]; then
            continue
        fi
        if [ "${#wanted[@]}" -gt 0 ]; then
            keep=false
            for want in "${wanted[@]}"; do
                if [ "$name" = "$want" ]; then
                    keep=true
                    break
                fi
            done
            if [ "$keep" != true ]; then
                continue
            fi
        fi
        BENCH_PAIRS+=("${package}"$'\t'"${name}")
    done < <(criterion_bench_pairs)

    if [ "${#BENCH_PAIRS[@]}" -eq 0 ]; then
        log_error "No criterion bench targets found in the workspace"
        return 1
    fi

    for want in "${wanted[@]}"; do
        found=false
        for entry in "${BENCH_PAIRS[@]}"; do
            if [ "${entry#*$'\t'}" = "$want" ]; then
                found=true
                break
            fi
        done
        if [ "$found" != true ]; then
            log_error "Requested bench target not found: ${want}"
            return 1
        fi
    done

    log_info "Criterion bench targets: ${#BENCH_PAIRS[@]}"
    for entry in "${BENCH_PAIRS[@]}"; do
        log_info "  -p ${entry%%$'\t'*} --bench ${entry#*$'\t'}"
    done
}

# Runs every selected target individually and fails when a target writes no
# `<bench>/new/estimates.json`. Without that assertion a stale/renamed target
# would silently measure nothing and leave the regression gate permanently
# green — worse than a red gate.
#
# Arguments are extra criterion flags appended after `--` (for example
# `--save-baseline baseline`).
run_bench_targets() {
    local extra_flags="${1:-}"
    local -a silent_targets=()
    local entry package name produced
    # Timestamp marker: `find -newer` then tells us whether the target actually
    # wrote fresh estimates. It lives under target/ so no temp-file cleanup is
    # needed (and so `rm` is never invoked on an MSYS /tmp path).
    local stamp="${PROJECT_ROOT}/target/.bench-progress-stamp"

    mkdir -p "$BASELINE_DIR"
    : > "$stamp"
    for entry in "${BENCH_PAIRS[@]}"; do
        package="${entry%%$'\t'*}"
        name="${entry#*$'\t'}"

        # Refresh the marker immediately before the target runs.
        : > "$stamp"
        if [ -n "$extra_flags" ]; then
            cargo bench -p "$package" --bench "$name" --no-fail-fast -- $extra_flags
        else
            cargo bench -p "$package" --bench "$name" --no-fail-fast
        fi
        produced="$(find "$BASELINE_DIR" -path '*/new/estimates.json' -newer "$stamp" -print -quit)"

        if [ -z "$produced" ]; then
            log_error "Bench target ${package}::${name} produced no new estimates"
            silent_targets+=("${package}::${name}")
        fi
    done

    if [ "${#silent_targets[@]}" -gt 0 ]; then
        log_error "Bench targets that produced no estimates: ${silent_targets[*]}"
        return 1
    fi
}

save_baseline() {
    log_info "Running benchmarks to establish baseline..."
    cd "$PROJECT_ROOT"
    load_bench_pairs

    run_bench_targets "--save-baseline baseline"

    # Copy results
    mkdir -p "$RESULTS_DIR"
    if [ -d "$BASELINE_DIR" ]; then
        # Merge instead of `rm -rf` + `cp -r`: recursive deletes are refused in
        # sandboxed environments, and `cp -r src dst` would nest when dst
        # already exists. `src/.` copies the contents.
        mkdir -p "${RESULTS_DIR}/baseline"
        cp -a "$BASELINE_DIR/." "${RESULTS_DIR}/baseline/"
        log_info "Baseline saved to ${RESULTS_DIR}/baseline"
    else
        log_error "Criterion baseline directory not found"
        return 1
    fi
}

# Blocking gate: measure the fixed subset twice on the machine that is running
# the check, then compare the two passes.
#
# Criterion's absolute timings are only meaningful within one machine and one
# load condition. Comparing against a baseline restored from an `actions/cache`
# entry means comparing a 2-core runner against whatever host produced that
# entry, which is how a no-op change ends up "10% slower". Recording the
# baseline in the same job removes the variable entirely.
run_gate() {
    cd "$PROJECT_ROOT"
    mkdir -p "$RESULTS_DIR"

    load_bench_pairs "${GATE_BENCHES[@]}"

    # Criterion keeps `change/` directories from earlier runs. Rather than
    # deleting them (a bulk recursive delete is refused outright in sandboxed
    # environments) the analyzer is told to ignore anything older than this.
    local started_at
    started_at="$(date +%s)"

    log_info "Pass 1/2: recording the baseline on this machine..."
    run_bench_targets "--save-baseline gatebase"

    log_info "Pass 2/2: measuring against the in-job baseline..."
    run_bench_targets "--baseline gatebase"

    analyze_results "$started_at"
}

run_comparison() {
    log_info "Running benchmarks with comparison..."
    cd "$PROJECT_ROOT"

    if [ ! -d "${RESULTS_DIR}/baseline" ]; then
        log_error "No baseline found. Run with --save-baseline first."
        exit 1
    fi

    # Merge the stored baseline back in (no recursive delete: see save_baseline).
    mkdir -p "$BASELINE_DIR"
    cp -a "${RESULTS_DIR}/baseline/." "$BASELINE_DIR/"

    load_bench_pairs

    # Run benchmarks with comparison
    run_bench_targets "--baseline baseline"

    log_info "Benchmark comparison complete"
}

analyze_results() {
    log_info "Analyzing benchmark results for regressions..."

    local changed_since="${1:-}"

    local baseline_dir="${RESULTS_DIR}/baseline"
    if [ ! -d "$baseline_dir" ]; then
        # Gate mode has no stored baseline: criterion already compared against
        # the in-job baseline, and the analyzer only reads `change/` files.
        baseline_dir="$BASELINE_DIR"
    fi

    local -a extra=()
    if [ -n "$changed_since" ]; then
        extra=(--changed-since "$changed_since")
    fi

    local status=0
    python3 "${SCRIPT_DIR}/analyze-bench.py" \
        --baseline "$baseline_dir" \
        --current "$BASELINE_DIR" \
        --threshold "$THRESHOLD_PERCENT" \
        --min-abs-ns "$MIN_ABS_NS" \
        --output "$REPORT_FILE" \
        "${extra[@]}" || status=$?

    case "$status" in
        0)
            log_info "No performance regressions above ${THRESHOLD_PERCENT}%"
            return 0
            ;;
        1)
            log_error "Performance regressions detected (see ${REPORT_FILE})"
            return 1
            ;;
        *)
            # Fail closed. A gate that cannot produce a verdict must not report
            # success: a missing report would otherwise look like "no
            # regressions" forever.
            log_error "Benchmark analyzer failed (status ${status}); cannot evaluate the gate"
            return 1
            ;;
    esac
}

# Parse command line arguments
GATE=false
SAVE_BASELINE=false
COMPARE=false
VERBOSE=false

while [[ $# -gt 0 ]]; do
    case $1 in
        --gate)
            GATE=true
            shift
            ;;
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
if [ "$GATE" = true ]; then
    run_gate
elif [ "$SAVE_BASELINE" = true ]; then
    save_baseline
elif [ "$COMPARE" = true ]; then
    run_comparison
    analyze_results
else
    log_error "No action specified. Use --gate, --save-baseline or --compare"
    usage
    exit 1
fi

log_info "Done!"
