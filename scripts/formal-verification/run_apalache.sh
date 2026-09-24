#!/usr/bin/env bash
# Apalache model-checking for neo-rs parallel-execution / reference-counter safety.
# Runs 4 checks: correct protocol (expect PASS) and a deliberately buggy variant
# (expect a counterexample proving each invariant is non-vacuous).
set -u
export PATH="/c/Program Files/Eclipse Adoptium/jre-21.0.12.101-hotspot/bin:$PATH"
APALACHE="$(cd "$(dirname "$0")/../.." && pwd)/.tools/apalache-0.62.2/bin/apalache-mc"
MODEL_DIR="$(cd "$(dirname "$0")/../../formal/consensus/model" && pwd)"
cd "$MODEL_DIR" || exit 2

run() {
  local label="$1"; shift
  echo "===== $label ====="
  "$APALACHE" check --length=10 "$@" 2>&1 \
    | grep -iE "Checker reports no error|violat|error|counterexample|OK|EXCEPTION" | head -8
  echo
}

run "refcount  correct  (NoCollectWhileReachable holds)" \
    --init=Init --next=Next --inv=NoCollectWhileReachable neo_refcount.tla
run "refcount  buggy    (NoCollectWhileReachable violated)" \
    --init=Init --next=BuggyNext --inv=NoCollectWhileReachable neo_refcount.tla
run "parallel  disjoint (SerialEquivalence holds)" \
    --init=Init --next=Next --inv=SerialEquivalence neo_parallel_exec.tla
run "parallel  conflict (SerialEquivalence violated)" \
    --init=Init --next=ConflictNext --inv=SerialEquivalence neo_parallel_exec.tla