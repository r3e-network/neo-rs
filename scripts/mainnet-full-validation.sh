#!/usr/bin/env bash
#
# mainnet-full-validation.sh — resumable orchestration for the MainNet [0, H]
# state-root validation campaign.
#
# Pipeline (each step is independently re-enterable):
#   1. verify-acc   Verify the downloaded `chain.0.acc.zip` MD5. Skipped when a
#                   matching marker (`<zip>.md5-ok`) exists.
#   2. unzip        Extract the inner `.acc` payload. Skipped when the extracted
#                   `.acc` already exists.
#   3. import       `neo-node --import-acc <acc> --import-only`. The importer is
#                   itself resumable (it skips blocks <= current height), and we
#                   also write a completion marker so a clean re-run is skipped.
#   4. height       Print the imported tip height (parsed from the import log).
#   5. node-start   Launch the validation node in the background and wait until
#                   its RPC answers `getblockcount`.
#   6. verify       Run `scripts/mainnet-full-verify.py` (fail-closed compare).
#   7. campaign     DENSE lane: gate-based in-import verification (see below).
#
# Two lanes (see docs/PROTOCOL_CONSISTENCY.md "RPC limiter + two-lane design"):
#   * DENSE  = `campaign`. The importer runs with data/reference_stateroots.jsonl
#     present; neo::state_service compares the computed root to the C# reference
#     at EVERY executed height in-process (not rate-limited) and aborts the
#     import at the first divergence. The runner turns that into a fail-closed
#     JSON report with the maximal verified contiguous prefix. Expect an abort.
#   * SAMPLED = `verify`. RPC getstateroot vs live C# seeds, stride >= 100,
#     --parallel <= 40 (the local RPC is capped at 100 rps and 40 connections).
#
# Usage:
#   scripts/mainnet-full-validation.sh [all|<step>] [--no-start-node]
#
# Environment overrides:
#   NEO_NODE_BIN           path to the neo-node binary (default: target/release/neo-node.exe)
#   NEO_IMPORT_STOP_HEIGHT optional early-stop height for a bounded smoke import
#   NEO_ROCKSDB_BATCH_PROFILE  import batch profile (default: high-throughput)
#   NODE_EXTRA_ARGS        extra flags appended to the node launch (e.g. pin a
#                          dead seed to keep an offline verify node off the network)
#   VERIFY_START / VERIFY_END  height range for the `verify` step
#   VERIFY_PARALLEL        verifier worker threads
#   CAMPAIGN_START/END     dense-lane range (default 0 .. 13141249 = dump end)
#   CAMPAIGN_STORE         dense-lane storage dir (default $DATA_DIR)
#   CAMPAIGN_PLUGINS_DIR   isolated plugins dir (exported as NEO_PLUGINS_DIR) so
#                          the gate run does not share the StateService MPT store
#   REFERENCE_FILE         C# reference jsonl (default data/reference_stateroots.jsonl)
#   CAMPAIGN_IMPORT_LOG / CAMPAIGN_GATE_LOG / CAMPAIGN_REPORT_DIR  dense-lane I/O
#   EXPECTED_STOPS         comma list of known expected-stop heights
#
# A campaign run that aborts (first divergence) exits NON-ZERO and writes
# `<store>/.campaign-aborted`; only a clean full pass writes `.campaign-complete`.
# A partial/sampled range is never reported as verified contiguous.
#
# Exit codes: 0 success, non-zero on the first failing step (fail-closed).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# ---- Paths -----------------------------------------------------------------
CONFIG="${CONFIG:-config/mainnet-full-validation.toml}"
DATA_DIR="${DATA_DIR:-data/mainnet-full-validation}"
ACC_ZIP="${ACC_ZIP:-data/bootstrap/chain.0.acc.zip}"
ACC_FILE="${ACC_FILE:-data/bootstrap/chain.0.acc}"
ACC_MD5_EXPECTED="${ACC_MD5_EXPECTED:-998C67C307F16B2D3608C225B2E892AE}"
LOG_DIR="${LOG_DIR:-logs}"
IMPORT_LOG="${IMPORT_LOG:-${LOG_DIR}/mainnet-full-import.log}"
IMPORT_MARKER="${DATA_DIR}/.import-complete"
NODE_LOG="${NODE_LOG:-${LOG_DIR}/mainnet-full-node.log}"
NODE_PID_FILE="${NODE_PID_FILE:-${DATA_DIR}/.node.pid}"
RPC_URL="${RPC_URL:-http://127.0.0.1:10332}"
# Fan the C# reference getstateroot calls across all official seeds (the verifier
# splits on commas). Fan-out + keep-alive is what makes the dense sweep feasible.
REFERENCE_URL="${REFERENCE_URL:-http://seed1.neo.org:10332,http://seed2.neo.org:10332,http://seed3.neo.org:10332,http://seed4.neo.org:10332,http://seed5.neo.org:10332}"

NEO_NODE_BIN="${NEO_NODE_BIN:-target/release/neo-node.exe}"
[[ -x "$NEO_NODE_BIN" ]] || NEO_NODE_BIN="target/release/neo-node"
export NEO_ROCKSDB_BATCH_PROFILE="${NEO_ROCKSDB_BATCH_PROFILE:-high-throughput}"

VERIFY_START="${VERIFY_START:-0}"
VERIFY_END="${VERIFY_END:-13141249}"
# Two LOCAL-leg caps constrain this (neo-rpc):
#   * per-IP GCRA rate limiter: getstateroot is Standard tier = 100 rps / burst
#     200, enforced for 127.0.0.1 too -> the local leg is capped ~100 hps no
#     matter how many workers, and extra workers only add -32001 throttling.
#   * max_concurrent_connections = 40 in the generated RpcServer.json -> holding
#     more than 40 keep-alive sockets exceeds the node's connection cap.
# So keep <= 40 (we run keep-alive, one connection per worker). The verifier
# retries -32001 with backoff, but backoff only burns quota; <=40 is the lever.
VERIFY_PARALLEL="${VERIFY_PARALLEL:-32}"

# ---- Dense (gate-based) campaign lane --------------------------------------
CAMPAIGN_START="${CAMPAIGN_START:-0}"
CAMPAIGN_END="${CAMPAIGN_END:-13141249}"
CAMPAIGN_STORE="${CAMPAIGN_STORE:-$DATA_DIR}"
# The node loads its reference file from a HARD-CODED relative path
# (neo-core/src/neo_system/storage.rs -> "data/reference_stateroots.jsonl"), so
# the runner must be invoked from the repo root and this path must match it.
REFERENCE_FILE="${REFERENCE_FILE:-data/reference_stateroots.jsonl}"
CAMPAIGN_PLUGINS_DIR="${CAMPAIGN_PLUGINS_DIR:-}"
CAMPAIGN_IMPORT_LOG="${CAMPAIGN_IMPORT_LOG:-${LOG_DIR}/mainnet-campaign-import.log}"
CAMPAIGN_GATE_LOG="${CAMPAIGN_GATE_LOG:-${LOG_DIR}/mainnet-campaign-refgate.log}"
CAMPAIGN_REPORT_DIR="${CAMPAIGN_REPORT_DIR:-outputs}"
CAMPAIGN_COMPLETE_MARKER="${CAMPAIGN_STORE}/.campaign-complete"
CAMPAIGN_ABORTED_MARKER="${CAMPAIGN_STORE}/.campaign-aborted"
EXPECTED_STOPS="${EXPECTED_STOPS:-5107,21373,980196,1465790}"

START_NODE=1

log() { echo "[$(date '+%F %T')] $*"; }
die() { echo "[$(date '+%F %T')] ERROR: $*" >&2; exit 1; }

file_size() {
  if stat --version >/dev/null 2>&1; then
    stat --format="%s" "$1"
  elif stat -f "%z" "$1" >/dev/null 2>&1; then
    stat -f "%z" "$1"
  else
    wc -c <"$1" | tr -d '[:space:]'
  fi
}

file_md5() {
  if command -v md5sum >/dev/null 2>&1; then
    md5sum "$1" | awk '{print toupper($1)}'
  elif command -v md5 >/dev/null 2>&1; then
    md5 -q "$1" | tr '[:lower:]' '[:upper:]'
  else
    die "neither md5sum nor md5 is available"
  fi
}

rpc_ready() {
  python3 - "$RPC_URL" <<'PY' 2>/dev/null
import gzip, json, sys, urllib.request
url = sys.argv[1]
body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "getblockcount", "params": []}).encode()
req = urllib.request.Request(
    url, data=body,
    headers={"Content-Type": "application/json", "Accept-Encoding": "identity"},
)
try:
    with urllib.request.urlopen(req, timeout=5) as resp:
        raw = resp.read()
    # neo-rpc may reply with gzip even when identity is requested.
    if raw.startswith(b"\x1f\x8b"):
        raw = gzip.decompress(raw)
    body = json.loads(raw.decode("utf-8"))
    if body.get("error"):
        sys.exit(1)
    print(body.get("result"))
except Exception:
    sys.exit(1)
PY
}

# ---- Step 1: verify acc MD5 -------------------------------------------------
step_verify_acc() {
  [[ -f "$ACC_ZIP" ]] || die "acc zip not found: $ACC_ZIP (run scripts/download_acc_resume.sh)"
  local marker="${ACC_ZIP}.md5-ok"
  local size
  size="$(file_size "$ACC_ZIP")"

  if [[ -f "$marker" ]] && [[ "$(cat "$marker" 2>/dev/null)" == "${ACC_MD5_EXPECTED}:${size}" ]]; then
    log "verify-acc: SKIP (marker matches ${ACC_MD5_EXPECTED} size=${size})"
    return 0
  fi

  log "verify-acc: hashing $ACC_ZIP (${size} bytes) ..."
  local actual
  actual="$(file_md5 "$ACC_ZIP")"
  if [[ "$actual" != "$ACC_MD5_EXPECTED" ]]; then
    echo "$actual:${size}" >"${marker}.bad"
    die "acc MD5 mismatch: expected ${ACC_MD5_EXPECTED}, got ${actual}"
  fi
  printf '%s:%s' "$ACC_MD5_EXPECTED" "$size" >"$marker"
  log "verify-acc: OK md5=${actual}"
}

# ---- Step 2: unzip inner .acc ----------------------------------------------
step_unzip() {
  if [[ -f "$ACC_FILE" ]]; then
    log "unzip: SKIP (already extracted: $ACC_FILE, size=$(file_size "$ACC_FILE"))"
    return 0
  fi
  [[ -f "$ACC_ZIP" ]] || die "cannot unzip, missing $ACC_ZIP"
  log "unzip: testing archive integrity ..."
  unzip -tq "$ACC_ZIP" >/dev/null || die "zip integrity test failed (incomplete download?)"

  local entry
  entry="$(unzip -Z1 "$ACC_ZIP" | grep -iE '\.acc$' | head -n1 || true)"
  [[ -n "$entry" ]] || entry="chain.0.acc"
  log "unzip: extracting entry '${entry}' -> ${ACC_FILE}"

  local tmp="${ACC_FILE}.part"
  rm -f "$tmp"
  # `-p` streams to stdout; robust against path quirks inside the archive.
  unzip -p "$ACC_ZIP" "$entry" >"$tmp" || die "failed to extract ${entry}"
  mv -f "$tmp" "$ACC_FILE"
  log "unzip: OK size=$(file_size "$ACC_FILE")"
}

# ---- Step 3: import ---------------------------------------------------------
step_import() {
  if [[ -f "$IMPORT_MARKER" ]]; then
    log "import: SKIP (marker present: $(cat "$IMPORT_MARKER"))"
    return 0
  fi
  [[ -f "$ACC_FILE" ]] || die "missing extracted acc file: $ACC_FILE"
  [[ -x "$NEO_NODE_BIN" ]] || die "neo-node binary not found/executable: $NEO_NODE_BIN"

  mkdir -p "$LOG_DIR" "$DATA_DIR"
  log "import: starting (log -> ${IMPORT_LOG})"
  local started
  started="$(date +%s)"

  # Unset any inherited NEO_IMPORT_ACC so only the explicit flag selects the file.
  if env -u NEO_IMPORT_ACC "$NEO_NODE_BIN" --config "$CONFIG" \
      --import-acc "$ACC_FILE" --import-only 2>&1 | tee -a "$IMPORT_LOG"; then
    local elapsed=$(( $(date +%s) - started ))
    local h
    h="$(tail -n 400 "$IMPORT_LOG" | grep -oE '"final_height":[0-9]+' | tail -n1 | grep -oE '[0-9]+' || true)"
    if [[ -n "${NEO_IMPORT_STOP_HEIGHT:-}" ]]; then
      # Bounded run: never write the completion marker, so the full import step
      # still runs later (the importer resumes from the current height).
      printf 'partial height=%s stop=%s elapsed_secs=%s at=%s\n' \
        "${h:-unknown}" "$NEO_IMPORT_STOP_HEIGHT" "$elapsed" "$(date -u '+%FT%TZ')" \
        >"${DATA_DIR}/.import-partial"
      log "import: PARTIAL (bounded) final_height=${h:-unknown} stop=$NEO_IMPORT_STOP_HEIGHT elapsed=${elapsed}s (no completion marker)"
    else
      printf 'height=%s elapsed_secs=%s at=%s\n' \
        "${h:-unknown}" "$elapsed" "$(date -u '+%FT%TZ')" >"$IMPORT_MARKER"
      log "import: OK final_height=${h:-unknown} elapsed=${elapsed}s"
    fi
  else
    die "import failed (see ${IMPORT_LOG})"
  fi
}

# ---- Step 4: report height --------------------------------------------------
step_height() {
  [[ -f "$IMPORT_MARKER" ]] || log "height: no marker yet (import incomplete)"
  if [[ -f "$IMPORT_MARKER" ]]; then
    log "height: $(cat "$IMPORT_MARKER")"
  fi
  if rpc_ready >/dev/null 2>&1; then
    log "height: RPC tip = $(rpc_ready)"
  fi
}

# ---- Step 5: start node -----------------------------------------------------
step_node_start() {
  if rpc_ready >/dev/null 2>&1; then
    log "node-start: SKIP (RPC already answering at ${RPC_URL})"
    return 0
  fi
  [[ -x "$NEO_NODE_BIN" ]] || die "neo-node binary not found/executable: $NEO_NODE_BIN"
  mkdir -p "$LOG_DIR" "$DATA_DIR"

  log "node-start: launching (pid -> ${NODE_PID_FILE}, log -> ${NODE_LOG})"
  # NODE_EXTRA_ARGS lets an offline verification run pin a dead seed and stay at
  # the imported height (e.g. NODE_EXTRA_ARGS="--seed 127.0.0.1:9 --min-connections 0 --max-connections 1").
  local -a extra=()
  if [[ -n "${NODE_EXTRA_ARGS:-}" ]]; then
    read -r -a extra <<<"$NODE_EXTRA_ARGS"
  fi
  nohup "$NEO_NODE_BIN" --config "$CONFIG" "${extra[@]}" >>"$NODE_LOG" 2>&1 &
  echo "$!" >"$NODE_PID_FILE"

  local waited=0
  while (( waited < 120 )); do
    if rpc_ready >/dev/null 2>&1; then
      log "node-start: RPC ready after ${waited}s (tip=$(rpc_ready))"
      return 0
    fi
    sleep 2
    waited=$(( waited + 2 ))
  done
  die "node RPC did not become ready within 120s (see ${NODE_LOG})"
}

# ---- Step 6: verify ---------------------------------------------------------
step_verify() {
  if (( VERIFY_PARALLEL > 40 )); then
    log "verify: WARN VERIFY_PARALLEL=${VERIFY_PARALLEL} exceeds the local RPC max_concurrent_connections (40) and the 100 rps Standard-tier limiter; expect -32001 throttling and connections refused. 32-40 is the sweet spot for the local leg."
  fi
  log "verify: comparing local ${RPC_URL} vs reference ${REFERENCE_URL} over [${VERIFY_START}, ${VERIFY_END}]"
  python3 scripts/mainnet-full-verify.py \
    --local "$RPC_URL" \
    --reference "$REFERENCE_URL" \
    --start "$VERIFY_START" \
    --end "$VERIFY_END" \
    --parallel "$VERIFY_PARALLEL" \
    --checkpoint "${DATA_DIR}/verify-checkpoint.json" \
    --report-dir outputs
}

# ---- Step 7: dense gate-based campaign --------------------------------------
# Precondition-gates the C# reference file, runs the import with the reference
# present (the in-process gate aborts at the first divergence), and emits a
# fail-closed JSON report. Exits non-zero unless the range passed cleanly.
step_campaign() {
  [[ -f "$ACC_FILE" ]] || die "campaign: missing extracted acc file: $ACC_FILE (run: $0 unzip)"
  [[ -x "$NEO_NODE_BIN" ]] || die "campaign: neo-node binary not found/executable: $NEO_NODE_BIN"
  [[ -f "$REFERENCE_FILE" ]] || die "campaign: missing reference file: $REFERENCE_FILE (run scripts/download_reference_roots.py)"
  mkdir -p "$LOG_DIR" "$CAMPAIGN_REPORT_DIR" "$CAMPAIGN_STORE"

  log "campaign: range [${CAMPAIGN_START}, ${CAMPAIGN_END}] store=${CAMPAIGN_STORE} reference=${REFERENCE_FILE}"

  # 1. Precondition gate: reference file must cover the range completely.
  log "campaign: reference completeness gate ..."
  local gate_rc=0
  set +e
  python3 scripts/download_reference_roots.py --verify-only \
    --start "$CAMPAIGN_START" --end "$CAMPAIGN_END" --output "$REFERENCE_FILE" \
    >"$CAMPAIGN_GATE_LOG" 2>&1
  gate_rc=$?
  set -e

  local gate_ok="true"
  if ! python3 scripts/mainnet_gate_report.py --gate-only \
      --gate-output "$CAMPAIGN_GATE_LOG" \
      --range-start "$CAMPAIGN_START" --range-end "$CAMPAIGN_END" >/dev/null 2>&1; then
    gate_ok="false"
  fi
  local ref_count
  ref_count="$(wc -l <"$REFERENCE_FILE" 2>/dev/null | tr -d '[:space:]')"
  log "campaign: gate verify-only exit=${gate_rc} effective_pass=${gate_ok} reference_records=${ref_count:-0}"

  # 2. Import with the reference present. It aborts at the first divergence by
  #    design; that abort is the mechanism, never swallowed as success.
  local import_rc=127  # 127 = import never ran (gate refused)
  if [[ "$gate_ok" == "true" ]]; then
    log "campaign: import starting (expect abort at first divergence; log -> ${CAMPAIGN_IMPORT_LOG})"
    local -a store_args=() env_args=(env -u NEO_IMPORT_ACC)
    [[ "$CAMPAIGN_STORE" != "$DATA_DIR" ]] && store_args=(--storage "$CAMPAIGN_STORE")
    [[ -n "$CAMPAIGN_PLUGINS_DIR" ]] && env_args+=(NEO_PLUGINS_DIR="$CAMPAIGN_PLUGINS_DIR")
    if [[ -n "${NEO_IMPORT_STOP_HEIGHT:-}" ]]; then
      log "campaign: bounded run - NEO_IMPORT_STOP_HEIGHT=${NEO_IMPORT_STOP_HEIGHT}"
    fi
    if [[ -n "$CAMPAIGN_PLUGINS_DIR" ]]; then
      log "campaign: isolated plugins dir NEO_PLUGINS_DIR=${CAMPAIGN_PLUGINS_DIR}"
    fi
    : >>"$CAMPAIGN_IMPORT_LOG"
    set +e
    "${env_args[@]}" "$NEO_NODE_BIN" --config "$CONFIG" \
      "${store_args[@]}" \
      --import-acc "$ACC_FILE" --import-only 2>&1 | tee -a "$CAMPAIGN_IMPORT_LOG"
    import_rc="${PIPESTATUS[0]}"
    set -e
    log "campaign: import exit=${import_rc}"
  else
    log "campaign: REFUSING to import - reference file incomplete (see ${CAMPAIGN_GATE_LOG})"
  fi

  # 3. Fail-closed JSON report.
  local stamp out report_rc=0
  stamp="$(date -u '+%Y%m%dT%H%M%SZ')"
  out="${CAMPAIGN_REPORT_DIR}/mainnet-campaign-${CAMPAIGN_START}-${CAMPAIGN_END}-${stamp}.json"
  set +e
  python3 scripts/mainnet_gate_report.py \
    --gate-output "$CAMPAIGN_GATE_LOG" \
    --range-start "$CAMPAIGN_START" --range-end "$CAMPAIGN_END" \
    --import-log "$CAMPAIGN_IMPORT_LOG" \
    --import-exit-code "$import_rc" \
    --gate-ok "$gate_ok" \
    --reference-file "$REFERENCE_FILE" \
    --reference-count "${ref_count:-0}" \
    --config "$CONFIG" --store "$CAMPAIGN_STORE" --acc-file "$ACC_FILE" \
    --expected-stops "$EXPECTED_STOPS" \
    --out "$out"
  report_rc=$?
  set -e

  # 4. Marker discipline: a success marker is written ONLY on a clean full pass.
  if [[ "$report_rc" -eq 0 ]]; then
    printf 'clean range=[%s,%s] at=%s\n' \
      "$CAMPAIGN_START" "$CAMPAIGN_END" "$(date -u '+%FT%TZ')" >"$CAMPAIGN_COMPLETE_MARKER"
    rm -f "$CAMPAIGN_ABORTED_MARKER"
    log "campaign: CLEAN full pass -> ${CAMPAIGN_COMPLETE_MARKER}"
  else
    printf 'status=not-clean imported_exit=%s range=[%s,%s] at=%s\n' \
      "$import_rc" "$CAMPAIGN_START" "$CAMPAIGN_END" "$(date -u '+%FT%TZ')" >"$CAMPAIGN_ABORTED_MARKER"
    # A run that did not pass cleanly must not leave any success marker behind.
    rm -f "$CAMPAIGN_COMPLETE_MARKER" "$IMPORT_MARKER"
    log "campaign: NOT CLEAN (exit=${report_rc}) -> ${CAMPAIGN_ABORTED_MARKER}"
  fi

  return "$report_rc"
}

# ---- Driver -----------------------------------------------------------------
run_step() {
  case "$1" in
    verify-acc) step_verify_acc ;;
    unzip)      step_unzip ;;
    import)     step_import ;;
    height)     step_height ;;
    node-start) step_node_start ;;
    verify)     step_verify ;;
    campaign)   step_campaign ;;
    all)
      step_verify_acc
      step_unzip
      step_import
      step_height
      if (( START_NODE )); then
        step_node_start
      fi
      step_verify
      ;;
    *) die "unknown step: $1" ;;
  esac
}

main() {
  local target="all"
  local arg
  for arg in "$@"; do
    case "$arg" in
      --no-start-node) START_NODE=0 ;;
      -h|--help) sed -n '2,52p' "$0"; return 0 ;;
      *) target="$arg" ;;
    esac
  done
  log "=== mainnet-full-validation: step=${target} ==="
  if run_step "$target"; then
    log "=== mainnet-full-validation: step=${target} DONE ==="
  else
    local rc=$?
    log "=== mainnet-full-validation: step=${target} exit=${rc} ==="
    exit "$rc"
  fi
}

main "$@"
