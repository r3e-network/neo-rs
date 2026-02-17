#!/usr/bin/env bash
set -euo pipefail

RPC_URL="${RPC_URL:-http://127.0.0.1:20332}"
STALL_SECONDS="${STALL_SECONDS:-180}"
POLL_SECONDS="${POLL_SECONDS:-15}"
NODE_LOG="${NODE_LOG:-logs/neo-node-watchdog-node.log}"
WATCHDOG_LOG="${WATCHDOG_LOG:-logs/neo-node-watchdog.log}"
RUST_LOG_LEVEL="${RUST_LOG_LEVEL:-warn}"
ROCKSDB_BATCH_PROFILE="${ROCKSDB_BATCH_PROFILE:-balanced}"
NODE_CMD=(./target/release/neo-node --config neo_testnet_node.toml)

mkdir -p "$(dirname "$NODE_LOG")" "$(dirname "$WATCHDOG_LOG")"

timestamp() {
  date '+%Y-%m-%d %H:%M:%S'
}

log() {
  printf '%s %s\n' "$(timestamp)" "$*" | tee -a "$WATCHDOG_LOG"
}

rpc_blockcount() {
  curl --compressed -s -H 'Content-Type: application/json' \
    -d '{"jsonrpc":"2.0","method":"getblockcount","params":[],"id":1}' \
    "$RPC_URL" | jq -r '.result // empty'
}

rpc_headercount() {
  curl --compressed -s -H 'Content-Type: application/json' \
    -d '{"jsonrpc":"2.0","method":"getblockheadercount","params":[],"id":1}' \
    "$RPC_URL" | jq -r '.result // empty'
}

start_node() {
  # Stop leftover instance before launching a new one.
  pkill -f 'target/release/neo-node --config neo_testnet_node.toml' >/dev/null 2>&1 || true
  env \
    RUST_LOG="$RUST_LOG_LEVEL" \
    NEO_ROCKSDB_BATCH_PROFILE="$ROCKSDB_BATCH_PROFILE" \
    "${NODE_CMD[@]}" >>"$NODE_LOG" 2>&1 &
  NODE_PID=$!
  log "node started pid=$NODE_PID"
}

stop_node() {
  if [[ -n "${NODE_PID:-}" ]] && kill -0 "$NODE_PID" >/dev/null 2>&1; then
    kill "$NODE_PID" >/dev/null 2>&1 || true
    wait "$NODE_PID" 2>/dev/null || true
    log "node stopped pid=$NODE_PID"
  fi
}

cleanup() {
  stop_node
}

trap cleanup EXIT INT TERM

NODE_PID=""
last_height=""
last_header=""
last_progress_ts=0

log "watchdog config poll=${POLL_SECONDS}s stall=${STALL_SECONDS}s rocksdb_profile=${ROCKSDB_BATCH_PROFILE}"
start_node
last_progress_ts=$(date +%s)

while true; do
  now_ts=$(date +%s)

  if ! kill -0 "$NODE_PID" >/dev/null 2>&1; then
    log "node process exited unexpectedly pid=$NODE_PID, restarting"
    start_node
    last_progress_ts=$now_ts
    last_height=""
    last_header=""
    sleep "$POLL_SECONDS"
    continue
  fi

  height="$(rpc_blockcount || true)"
  header="$(rpc_headercount || true)"
  if [[ "$height" =~ ^[0-9]+$ ]] && [[ "$header" =~ ^[0-9]+$ ]]; then
    if [[ "$height" != "$last_height" ]] || [[ "$header" != "$last_header" ]]; then
      log "progress height=$height header=$header"
      last_height="$height"
      last_header="$header"
      last_progress_ts=$now_ts
    elif (( now_ts - last_progress_ts >= STALL_SECONDS )); then
      log "stall detected at height=$height header=$header for $((now_ts - last_progress_ts))s, restarting node"
      stop_node
      start_node
      last_progress_ts=$now_ts
      last_height=""
      last_header=""
    fi
  elif (( now_ts - last_progress_ts >= STALL_SECONDS )); then
    log "rpc unavailable for $((now_ts - last_progress_ts))s, restarting node"
    stop_node
    start_node
    last_progress_ts=$now_ts
    last_height=""
    last_header=""
  fi

  sleep "$POLL_SECONDS"
done
