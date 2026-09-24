#!/usr/bin/env bash
# Run TLC on the minimal consensus model for both the healthy (n=4,f=1) and
# unhealthy (n=3,f=1) scenarios.  Requires: Java 21 (or any JRE >= 11) and
# tla2tools.jar.  Paths are configurable below.
set -euo pipefail

JAVA="${JAVA:-/c/Program Files/Eclipse Adoptium/jre-21.0.12.101-hotspot/bin/java.exe}"
JAR="${JAR:-$(cd "$(dirname "$0")/../.." && pwd)/.tools/tla2tools.jar}"
MODEL_DIR="$(cd "$(dirname "$0")/../../formal/consensus/model" && pwd)"

echo "== Healthy  (n=4, f=1) =="
(cd "$MODEL_DIR" && "$JAVA" -XX:+UseParallelGC -jar "$JAR" minimal_consensus.tla -config tlc_min_healthy.cfg -workers 2) \
  | grep -iE "Model checking completed|states generated|Error|violat"
echo
echo "== Unhealthy (n=3, f=1) =="
(cd "$MODEL_DIR" && "$JAVA" -XX:+UseParallelGC -jar "$JAR" minimal_consensus.tla -config tlc_min_unhealthy.cfg -workers 2) \
  | grep -iE "Model checking completed|states generated|Error|violat"