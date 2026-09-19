#!/usr/bin/env bash
# Offline contract tests. Stub output is deliberately synthetic, not proof evidence.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
mkdir "$WORK/model"
cp "$HERE/../consensus/model/neo_dbft_complete.tla" "$WORK/model/"
for cfg in tlc_neo_4f1 tlc_neo_7f2 tlc_neo_3f1_M3; do
  cp "$HERE/../consensus/model/$cfg.cfg" "$WORK/model/"
done
: > "$WORK/tool.jar"
# Keep the stub in this test source, independent of production output parsing.
# printf writes it verbatim (no shell interpolation until the stub executes).
printf '%s\n' '#!/usr/bin/env bash
set -eu
if [ "$1" = "-version" ]; then
  if [ "$CASE" = "version-failure" ]; then exit 99; fi
  printf "synthetic Java version\n"
  exit 0
fi
cfg=""
while [ "$#" -gt 0 ]; do
  if [ "$1" = "-config" ]; then shift; cfg="$1"; fi
  shift
done
if [ "$cfg" = "nonvacuity.cfg" ]; then
  [ "$CASE" != "clean-witness" ] || exit 0
  [ "$CASE" != "crash-witness" ] || { printf "OutOfMemoryError\n"; exit 255; }
  if [ "$CASE" = "wrong-invariant" ]; then
    printf "Error: Invariant TypeOK is violated.\n"
  else
    printf "Error: Invariant NoFinalization is violated.\n"
  fi
  if [ "$CASE" != "missing-trace" ]; then
    printf "State 7: <Finalize line 53, col 5 to line 57, col 55 of module neo_dbft_complete>\n"
  fi
  printf "231 states generated, 108 distinct states found, 35 states left on queue.\n"
  [ "$CASE" != "incomplete-witness" ] && printf "Finished in 00s\n"
  [ "$CASE" != "wrong-exit" ] || exit 1
  exit 12
fi
if [ "$cfg" = "tlc_neo_7f2.cfg" ]; then
  case "$CASE" in
    safe-crash) exit 255 ;;
    empty-success) exit 0 ;;
    safe-error) printf "Error: unrelated tool failure\n" ;;
    safe-frontier) printf "Model checking completed. No error has been found.\n10 distinct states found, 2 states left on queue.\n"; exit 0 ;;
  esac
fi
printf "Model checking completed. No error has been found.\n16073 states generated, 4276 distinct states found, 0 states left on queue.\nFinished in 00s\n"
exit 0
' > "$WORK/java"
chmod +x "$WORK/java"
export MODEL_DIR="$WORK/model" TLA2TOOLS_JAR="$WORK/tool.jar" JAVA_BIN="$WORK/java"
count=0
for CASE in pass safe-frontier safe-error safe-crash empty-success clean-witness wrong-exit crash-witness wrong-invariant missing-trace incomplete-witness version-failure; do
  export CASE ARTIFACT_DIR="$WORK/$CASE"
  rc=0
  bash "$HERE/run_tlc_ci.sh" > "$WORK/$CASE.output" 2>&1 || rc=$?
  if [ "$CASE" = pass ]; then
    [ "$rc" -eq 0 ]
    grep -qxF 'overall_exit: 0' "$ARTIFACT_DIR/summary.txt"
    for cfg in tlc_neo_4f1 tlc_neo_7f2 tlc_neo_3f1_M3 nonvacuity; do
      [ -s "$ARTIFACT_DIR/$cfg.log" ] && [ -s "$ARTIFACT_DIR/$cfg.exit" ]
    done
    grep -q 'nonvacuity.cfg' "$ARTIFACT_DIR/inputs.sha256"
    grep -qxF 'INVARIANT NoFinalization' "$ARTIFACT_DIR/nonvacuity.cfg"
  else
    [ "$rc" -ne 0 ]
    if [ "$CASE" != version-failure ]; then
      grep -qxF 'overall_exit: 1' "$ARTIFACT_DIR/summary.txt"
    fi
  fi
  printf 'PASS: %s (exit %s)\n' "$CASE" "$rc"
  count=$((count+1))
done
export CASE=pass
for negative in missing-java missing-jar missing-config stale-evidence; do
  export ARTIFACT_DIR="$WORK/$negative"
  rc=0
  case "$negative" in
    missing-java) JAVA_BIN="$WORK/absent" bash "$HERE/run_tlc_ci.sh" > "$WORK/$negative.output" 2>&1 || rc=$? ;;
    missing-jar) TLA2TOOLS_JAR="$WORK/absent" bash "$HERE/run_tlc_ci.sh" > "$WORK/$negative.output" 2>&1 || rc=$? ;;
    missing-config)
      mv "$MODEL_DIR/tlc_neo_7f2.cfg" "$WORK/saved.cfg"
      bash "$HERE/run_tlc_ci.sh" > "$WORK/$negative.output" 2>&1 || rc=$?
      mv "$WORK/saved.cfg" "$MODEL_DIR/tlc_neo_7f2.cfg" ;;
    stale-evidence)
      mkdir "$ARTIFACT_DIR"
      bash "$HERE/run_tlc_ci.sh" > "$WORK/$negative.output" 2>&1 || rc=$? ;;
  esac
  [ "$rc" -ne 0 ]
  printf 'PASS: %s (exit %s)\n' "$negative" "$rc"
  count=$((count+1))
done
printf '%s contract scenarios passed\n' "$count"
