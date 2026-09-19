#!/usr/bin/env bash
# Bounded single-height/view quorum abstraction, NOT full protocol verification.
# Run in a fresh evidence workspace; never reuse model states or generated files.
# The witness adds NoFinalization to the n=4 safety config and must produce
# TLC's invariant-violation exit code AND the named counterexample trace.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
MODEL_DIR="${MODEL_DIR:-$ROOT/formal/consensus/model}"
ARTIFACT_DIR="${ARTIFACT_DIR:-$ROOT/reports/tlc}"
JAVA_BIN="${JAVA_BIN:-java}"
TLA2TOOLS_JAR="${TLA2TOOLS_JAR:-$ROOT/.tools/tla2tools.jar}"
TLC_WORKERS="${TLC_WORKERS:-2}"
SAFE_CONFIGS=(tlc_neo_4f1.cfg tlc_neo_7f2.cfg tlc_neo_3f1_M3.cfg)

die() { printf 'run_tlc_ci: %s\n' "$*" >&2; exit 1; }
command -v "$JAVA_BIN" >/dev/null || die "Java not found: $JAVA_BIN"
[ -f "$TLA2TOOLS_JAR" ] || die "Missing jar: $TLA2TOOLS_JAR"
TLA2TOOLS_JAR="$(cd "$(dirname "$TLA2TOOLS_JAR")" && pwd)/$(basename "$TLA2TOOLS_JAR")"
[ -f "$MODEL_DIR/neo_dbft_complete.tla" ] || die "Missing quorum model"
for cfg in "${SAFE_CONFIGS[@]}"; do
  [ -f "$MODEL_DIR/$cfg" ] || die "Missing config: $cfg"
done
# Refuse stale evidence, rather than accidentally uploading a previous success.
[ ! -e "$ARTIFACT_DIR" ] || die "Evidence directory already exists: $ARTIFACT_DIR"
mkdir -p "$ARTIFACT_DIR"
ARTIFACT_DIR="$(cd "$ARTIFACT_DIR" && pwd)"
cp "$MODEL_DIR/neo_dbft_complete.tla" "$ARTIFACT_DIR/"
for cfg in "${SAFE_CONFIGS[@]}"; do cp "$MODEL_DIR/$cfg" "$ARTIFACT_DIR/"; done
cp "$ARTIFACT_DIR/tlc_neo_4f1.cfg" "$ARTIFACT_DIR/nonvacuity.cfg"
printf '\nINVARIANT NoFinalization\n' >> "$ARTIFACT_DIR/nonvacuity.cfg"
cd "$ARTIFACT_DIR"
sha256sum "$TLA2TOOLS_JAR" neo_dbft_complete.tla ./*.cfg > inputs.sha256
"$JAVA_BIN" -version > java-version.log 2>&1
printf 'Bounded quorum abstraction only; n=3/f=1 exceeds configured fault tolerance.\n' > summary.txt

run_tlc() {
  local cfg="$1" rc=0
  # No simulation/depth/state constraints: explore each finite configuration.
  # Deterministic fingerprint selection/seed, fresh per-run TLC state directory.
  "$JAVA_BIN" -XX:+UseParallelGC -Xmx2g -jar "$TLA2TOOLS_JAR" \
    -workers "$TLC_WORKERS" -seed 1 -fp 0 -config "$cfg.cfg" \
    -metadir "states-$cfg" neo_dbft_complete.tla > "$cfg.log" 2>&1 || rc=$?
  printf '%s\n' "$rc" > "$cfg.exit"
  return "$rc"
}

overall=0
for cfg_file in "${SAFE_CONFIGS[@]}"; do
  cfg="${cfg_file%.cfg}"
  rc=0
  run_tlc "$cfg" || rc=$?
  if [ "$rc" -eq 0 ] \
    && grep -qF 'Model checking completed. No error has been found.' "$cfg.log" \
    && grep -qE '[1-9][0-9]* distinct states found, 0 states left on queue\.' "$cfg.log" \
    && ! grep -qE 'Error:|Exception|violated' "$cfg.log"; then
    printf '%s: PASS (complete bounded search)\n' "$cfg" >> summary.txt
  else
    printf '%s: FAIL (exit %s; see full log)\n' "$cfg" "$rc" >> summary.txt
    overall=1
  fi
done

rc=0
run_tlc nonvacuity || rc=$?
# TLC v1.7.4: VIOLATION_SAFETY = 12. Other nonzero exits are not witnesses.
if [ "$rc" -eq 12 ] \
  && grep -qFx 'Error: Invariant NoFinalization is violated.' nonvacuity.log \
  && grep -qE '^State [2-9][0-9]*: <Finalize ' nonvacuity.log \
  && grep -qE '^[1-9][0-9]* states generated, [1-9][0-9]* distinct states found' nonvacuity.log \
  && grep -qF 'Finished in ' nonvacuity.log \
  && ! grep -qE 'Exception|OutOfMemory|Error: (TLC|Invariant (TypeOK|Invariant))' nonvacuity.log; then
  printf 'nonvacuity: PASS (NoFinalization counterexample, expected TLC exit 12)\n' >> summary.txt
else
  printf 'nonvacuity: FAIL (exit %s; named, completed trace required)\n' "$rc" >> summary.txt
  overall=1
fi
printf 'overall_exit: %s\n' "$overall" >> summary.txt
while IFS= read -r line; do printf '%s\n' "$line"; done < summary.txt
exit "$overall"
