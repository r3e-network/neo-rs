#!/usr/bin/env bash
# Run the faithful neo-rs dBFT model under TLC for the real (safe) neo constants
# and the boundary (unsafe) config, and report a machine-readable verdict.
set -u

JAVA="${JAVA:-/c/Program Files/Eclipse Adoptium/jre-21.0.12.101-hotspot/bin/java.exe}"
JAR="${JAR:-$(cd "$(dirname "$0")/../.." && pwd)/.tools/tla2tools.jar}"
MODEL_DIR="$(cd "$(dirname "$0")/../../formal/consensus/model" && pwd)"

run_cfg() {
  local name="$1"; shift
  echo "===== $name ====="
  local out
  out=$( (cd "$MODEL_DIR" && "$JAVA" -XX:+UseParallelGC -jar "$JAR" \
        neo_dbft.tla -config "$name.cfg" -workers 8) 2>&1 )
  echo "$out" | grep -iE "Model checking completed|states generated|Invariant|Error|violat|Deadlock" || true
  if echo "$out" | grep -qiE "violat|Error:"; then echo "VERDICT: $name -> FAIL/VIOLATION"; return 1
  elif echo "$out" | grep -qi "Model checking completed"; then echo "VERDICT: $name -> PASS"; return 0
  else echo "VERDICT: $name -> UNKNOWN"; return 2; fi
}

rc=0
run_cfg neo_dbft_safe_n4   || rc=1
echo
run_cfg neo_dbft_safe_n7   || rc=1
echo
run_cfg neo_dbft_boundary  || rc=1
exit $rc