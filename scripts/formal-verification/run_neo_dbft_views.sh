#!/usr/bin/env bash
# Phase A: neo-rs dBFT cross-view NoDoubleCommit / Agreement under TLC.
set -u
JAVA="${JAVA:-/c/Program Files/Eclipse Adoptium/jre-21.0.12.101-hotspot/bin/java.exe}"
JAR="${JAR:-$(cd "$(dirname "$0")/../.." && pwd)/.tools/tla2tools.jar}"
MODEL_DIR="$(cd "$(dirname "$0")/../../formal/consensus/model" && pwd)"
run_cfg() {
  echo "===== $1 ====="
  (cd "$MODEL_DIR" && "$JAVA" -XX:+UseParallelGC -jar "$JAR" neo_dbft_views.tla -config "$1.cfg" -workers 8) 2>&1 \
    | grep -iE "Model checking completed|states generated|Invariant .* violated|Error:|No error" || true
}
run_cfg views_safe_n4
echo
run_cfg views_safe_n7
echo
run_cfg views_boundary