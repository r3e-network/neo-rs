#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

CONFIG_PATH="neo_testnet_node.toml"
STORAGE_PATH="$ROOT/data/testnet-full"
BOOTSTRAP_DIR="$ROOT/data/bootstrap-testnet"
NODE_BIN="$ROOT/target/release/neo-node"
RPC_URL="http://127.0.0.1:20332"
REF_RPC_CSHARP="http://seed1t5.neo.org:20332"
REF_RPC_NEOGO="http://rpc.t5.n3.nspcc.ru:20332"
POLL_SECONDS=15
TARGET_LAG=0
MAX_WAIT_SECONDS=0
SKIP_BOOTSTRAP=0
LOG_FILE=""
IMPORT_LOG=""

usage() {
  cat <<'USAGE'
Usage: scripts/sync-testnet-full.sh [options]

Options:
  --config <path>           neo-node config path (default: neo_testnet_node.toml)
  --storage <path>          storage path for full sync (default: data/testnet-full)
  --bootstrap-dir <path>    directory for .acc package and extracted files
  --node-bin <path>         neo-node binary path (default: target/release/neo-node; falls back to debug)
  --rpc-url <url>           local RPC URL to validate (default: http://127.0.0.1:20332)
  --ref-csharp <url>        C# reference RPC (default: http://seed1t5.neo.org:20332)
  --ref-neogo <url>         NeoGo reference RPC (default: http://rpc.t5.n3.nspcc.ru:20332)
  --poll <sec>              polling interval (default: 15)
  --target-lag <n>          success when local lag <= n (default: 0)
  --max-wait <sec>          stop after this many seconds, 0 means no limit (default: 0)
  --skip-bootstrap          skip ACC download/import and just tail-sync current storage
  -h, --help                show this help
USAGE
}

extract_zip() {
  local zip_path="$1"
  local out_dir="$2"
  if command -v unzip >/dev/null 2>&1; then
    unzip -o "$zip_path" -d "$out_dir" >/dev/null
    return
  fi

  if command -v python3 >/dev/null 2>&1; then
    python3 - "$zip_path" "$out_dir" <<'PY'
import pathlib
import sys
import zipfile

zip_path = pathlib.Path(sys.argv[1])
out_dir = pathlib.Path(sys.argv[2])
out_dir.mkdir(parents=True, exist_ok=True)
with zipfile.ZipFile(zip_path) as zf:
    zf.extractall(out_dir)
PY
    return
  fi

  echo "error: cannot extract $zip_path (need unzip or python3)" >&2
  exit 1
}

while [[ $# -gt 0 ]]; do
  case "$1" in
  --config)
    CONFIG_PATH="$2"
    shift 2
    ;;
  --storage)
    STORAGE_PATH="$2"
    shift 2
    ;;
  --bootstrap-dir)
    BOOTSTRAP_DIR="$2"
    shift 2
    ;;
  --node-bin)
    NODE_BIN="$2"
    shift 2
    ;;
  --rpc-url)
    RPC_URL="$2"
    shift 2
    ;;
  --ref-csharp)
    REF_RPC_CSHARP="$2"
    shift 2
    ;;
  --ref-neogo)
    REF_RPC_NEOGO="$2"
    shift 2
    ;;
  --poll)
    POLL_SECONDS="$2"
    shift 2
    ;;
  --target-lag)
    TARGET_LAG="$2"
    shift 2
    ;;
  --max-wait)
    MAX_WAIT_SECONDS="$2"
    shift 2
    ;;
  --skip-bootstrap)
    SKIP_BOOTSTRAP=1
    shift
    ;;
  -h | --help)
    usage
    exit 0
    ;;
  *)
    echo "error: unknown argument: $1" >&2
    usage
    exit 1
    ;;
  esac
done

if ! command -v jq >/dev/null 2>&1; then
  echo "error: jq is required" >&2
  exit 1
fi
if ! command -v curl >/dev/null 2>&1; then
  echo "error: curl is required" >&2
  exit 1
fi

if [[ ! -x "$NODE_BIN" ]]; then
  if [[ "$NODE_BIN" == "$ROOT/target/release/neo-node" && -x "$ROOT/target/debug/neo-node" ]]; then
    NODE_BIN="$ROOT/target/debug/neo-node"
  else
    echo "error: neo-node binary not executable: $NODE_BIN" >&2
    exit 1
  fi
fi

rpc_call() {
  local method="$1"
  local params="${2:-[]}"
  curl --compressed -sS \
    -H 'content-type: application/json' \
    -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"${method}\",\"params\":${params}}" \
    "$RPC_URL"
}

rpc_ref_call() {
  local url="$1"
  local method="$2"
  local params="${3:-[]}"
  curl --compressed -sS \
    -H 'content-type: application/json' \
    -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"${method}\",\"params\":${params}}" \
    "$url"
}

timestamp() {
  date '+%Y-%m-%d %H:%M:%S'
}

log() {
  printf '[%s] %s\n' "$(timestamp)" "$*"
}

pick_max_remote_height() {
  local h1 h2
  h1="$(rpc_ref_call "$REF_RPC_CSHARP" getblockcount 2>/dev/null | jq -r '.result // -1')"
  h2="$(rpc_ref_call "$REF_RPC_NEOGO" getblockcount 2>/dev/null | jq -r '.result // -1')"
  if [[ ! "$h1" =~ ^[0-9]+$ ]]; then
    h1=-1
  fi
  if [[ ! "$h2" =~ ^[0-9]+$ ]]; then
    h2=-1
  fi
  if (( h1 > h2 )); then
    echo "$h1"
  else
    echo "$h2"
  fi
}

NODE_PID=""
cleanup() {
  if [[ -n "$NODE_PID" ]] && kill -0 "$NODE_PID" >/dev/null 2>&1; then
    kill -INT "$NODE_PID" >/dev/null 2>&1 || true
    wait "$NODE_PID" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

mkdir -p "$STORAGE_PATH" "$BOOTSTRAP_DIR"
LOG_FILE="$(mktemp /tmp/neo-testnet-full-sync-XXXX.log)"
IMPORT_LOG="$(mktemp /tmp/neo-testnet-import-XXXX.log)"

if [[ "$SKIP_BOOTSTRAP" -eq 0 ]]; then
  log "fetching latest TestNet ACC metadata"
  ACC_URL="$(curl -fsSL https://sync.ngd.network/config.json | jq -r '.n3testnet.full.path')"
  ACC_MD5="$(curl -fsSL https://sync.ngd.network/config.json | jq -r '.n3testnet.full.md5')"
  if [[ -z "$ACC_URL" || "$ACC_URL" == "null" || -z "$ACC_MD5" || "$ACC_MD5" == "null" ]]; then
    echo "error: failed to read n3testnet full ACC metadata" >&2
    exit 1
  fi

  ACC_ZIP="$BOOTSTRAP_DIR/chain.0.acc.zip"
  log "downloading ACC snapshot to $ACC_ZIP"
  "$ROOT/scripts/download_acc_resume.sh" "$ACC_URL" "$ACC_ZIP" "$ACC_MD5"

  log "extracting ACC snapshot"
  extract_zip "$ACC_ZIP" "$BOOTSTRAP_DIR"
  ACC_FILE="$(find "$BOOTSTRAP_DIR" -maxdepth 1 -type f -name '*.acc' | head -n 1)"
  if [[ -z "$ACC_FILE" ]]; then
    echo "error: no .acc file found in $BOOTSTRAP_DIR" >&2
    exit 1
  fi

  log "importing ACC snapshot into storage $STORAGE_PATH (this can take time)"
  "$NODE_BIN" \
    --config "$CONFIG_PATH" \
    --storage "$STORAGE_PATH" \
    --import-acc "$ACC_FILE" \
    --import-only >"$IMPORT_LOG" 2>&1
  log "ACC import complete"
else
  log "skipping bootstrap import (--skip-bootstrap)"
fi

log "starting neo-node for tail sync"
"$NODE_BIN" \
  --config "$CONFIG_PATH" \
  --storage "$STORAGE_PATH" \
  --logging-format json \
  --logging-level info >"$LOG_FILE" 2>&1 &
NODE_PID=$!

log "waiting for local RPC readiness at $RPC_URL"
for _ in $(seq 1 180); do
  if ! kill -0 "$NODE_PID" >/dev/null 2>&1; then
    echo "error: neo-node exited before RPC became ready" >&2
    tail -n 120 "$LOG_FILE" >&2 || true
    exit 1
  fi
  version_json="$(rpc_call getversion 2>/dev/null || true)"
  if jq -e '.result.protocol.network' >/dev/null 2>&1 <<<"$version_json"; then
    break
  fi
  sleep 1
done

network_magic="$(jq -r '.result.protocol.network // -1' <<<"$version_json")"
expected_magic=$((0x3554334E))
if [[ "$network_magic" != "$expected_magic" ]]; then
  echo "error: unexpected network magic $network_magic (expected $expected_magic for TestNet)" >&2
  exit 1
fi

start_ts="$(date +%s)"
NEO_HASH="0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5"
last_local=-1

while true; do
  now_ts="$(date +%s)"
  elapsed=$((now_ts - start_ts))

  if ! kill -0 "$NODE_PID" >/dev/null 2>&1; then
    echo "error: neo-node exited during sync loop" >&2
    tail -n 120 "$LOG_FILE" >&2 || true
    exit 1
  fi

  peers="$(rpc_call getconnectioncount | jq -r '.result // -1')"
  local_block="$(rpc_call getblockcount | jq -r '.result // -1')"
  local_header="$(rpc_call getblockheadercount | jq -r '.result // -1')"
  remote_block="$(pick_max_remote_height)"
  invoke_json="$(rpc_call invokefunction "[\"${NEO_HASH}\",\"totalSupply\",[]]")"
  invoke_state="$(jq -r '.result.state // ""' <<<"$invoke_json")"
  local_supply="$(jq -r '.result.stack[0].value // ""' <<<"$invoke_json")"

  if [[ ! "$peers" =~ ^[0-9]+$ || ! "$local_block" =~ ^[0-9]+$ || ! "$local_header" =~ ^[0-9]+$ ]]; then
    echo "error: failed to read local RPC counters" >&2
    exit 1
  fi
  if [[ ! "$remote_block" =~ ^[0-9]+$ || "$remote_block" -lt 1 ]]; then
    echo "error: failed to read remote reference height" >&2
    exit 1
  fi
  if [[ "$invoke_state" != "HALT" || -z "$local_supply" ]]; then
    echo "error: invokefunction totalSupply failed (state=$invoke_state)" >&2
    echo "$invoke_json" >&2
    exit 1
  fi

  lag=$((remote_block - local_block))
  progress=$((local_block - last_local))
  last_local="$local_block"

  csharp_supply="$(rpc_ref_call "$REF_RPC_CSHARP" invokefunction "[\"${NEO_HASH}\",\"totalSupply\",[]]" | jq -r '.result.stack[0].value // ""')"
  neogo_supply="$(rpc_ref_call "$REF_RPC_NEOGO" invokefunction "[\"${NEO_HASH}\",\"totalSupply\",[]]" | jq -r '.result.stack[0].value // ""')"
  if [[ -z "$csharp_supply" || -z "$neogo_supply" ]]; then
    echo "error: failed to query reference totalSupply values" >&2
    exit 1
  fi
  if [[ "$local_supply" != "$csharp_supply" || "$local_supply" != "$neogo_supply" ]]; then
    echo "error: totalSupply mismatch local=$local_supply csharp=$csharp_supply neogo=$neogo_supply" >&2
    exit 1
  fi

  log "sync status peers=$peers local_block=$local_block local_header=$local_header remote_block=$remote_block lag=$lag step_progress=$progress totalSupply=$local_supply"

  if (( lag <= TARGET_LAG )) && (( peers > 0 )) && (( local_header >= local_block )); then
    break
  fi

  if (( MAX_WAIT_SECONDS > 0 )) && (( elapsed >= MAX_WAIT_SECONDS )); then
    echo "error: max wait reached (${MAX_WAIT_SECONDS}s) before target lag ${TARGET_LAG}" >&2
    tail -n 120 "$LOG_FILE" >&2 || true
    exit 1
  fi

  sleep "$POLL_SECONDS"
done

# Sample one recent executed tx state once synced.
sample_tx_hash=""
sample_tx_vmstate=""
latest_index="$(rpc_call getblockcount | jq -r '.result // -1')"
latest_index=$((latest_index - 1))
for off in $(seq 0 500); do
  idx=$((latest_index - off))
  if (( idx < 0 )); then
    break
  fi
  block_json="$(rpc_call getblock "[${idx},true]")"
  tx_hash="$(jq -r '.result.tx[0].hash // ""' <<<"$block_json")"
  if [[ -n "$tx_hash" ]]; then
    sample_tx_hash="$tx_hash"
    applog_json="$(rpc_call getapplicationlog "[\"${tx_hash}\"]")"
    sample_tx_vmstate="$(jq -r '.result.executions[0].vmstate // ""' <<<"$applog_json")"
    break
  fi
done

echo
echo "TESTNET FULL SYNC VALIDATION SUCCEEDED"
echo "  network_magic:       $network_magic"
echo "  peers_connected:     $(rpc_call getconnectioncount | jq -r '.result // 0')"
echo "  local_block:         $(rpc_call getblockcount | jq -r '.result // -1')"
echo "  local_header:        $(rpc_call getblockheadercount | jq -r '.result // -1')"
echo "  remote_block(max):   $(pick_max_remote_height)"
echo "  total_supply(local): $(rpc_call invokefunction \"[\\\"${NEO_HASH}\\\",\\\"totalSupply\\\",[]]\" | jq -r '.result.stack[0].value // \"\"')"
echo "  sample_tx_hash:      ${sample_tx_hash:-<none-found>}"
echo "  sample_tx_vmstate:   ${sample_tx_vmstate:-<n/a>}"
echo "  storage_path:        $STORAGE_PATH"
echo "  import_log:          $IMPORT_LOG"
echo "  node_log:            $LOG_FILE"
