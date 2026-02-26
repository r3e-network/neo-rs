#!/usr/bin/env bash
set -euo pipefail

CONFIG_PATH="neo_mainnet_node.toml"
NODE_BIN="target/debug/neo-node"
TEE_DATA_PATH="/tmp/neo-tee-strict-test"
ORDERING_POLICY="batched"
RPC_URL="http://127.0.0.1:10332"
RPC_URL_EXPLICIT=0
RPC_USER="neo"
RPC_PASS="change-me-mainnet-rpc-password"
LISTEN_PORT=""
RPC_PORT=""
ITERATIONS=100
STARTUP_TIMEOUT=120
SYNC_WINDOW=30
BLOCK_PROGRESS_TIMEOUT=180
STORAGE_PATH=""
GENERATE_EVIDENCE=1
ALLOW_NON_TERMINAL_QV=0
ALLOW_EXPIRED_COLLATERAL=0
REQUIRE_BLOCK_PROGRESS=0

usage() {
  cat <<'USAGE'
Usage: scripts/validate-tee-sgx-runtime.sh [options]

Options:
  --config <path>                  Node config path (default: neo_mainnet_node.toml)
  --binary <path>                  neo-node binary path (default: target/debug/neo-node)
  --tee-data-path <path>           TEE data path with sgx.quote/sgx.sealing_key
  --storage <path>                 Optional storage path override
  --ordering-policy <policy>       TEE ordering policy (default: batched)
  --listen-port <port>             Override node P2P listen port for this run
  --rpc-port <port>                Override node RPC port for this run
  --rpc-url <url>                  RPC URL (default: http://127.0.0.1:10332)
  --rpc-user <user>                RPC basic auth user (default: neo)
  --rpc-pass <pass>                RPC basic auth pass
  --iterations <n>                 Repeated live validation loops (default: 100)
  --startup-timeout <sec>          RPC readiness timeout (default: 120)
  --sync-window <sec>              Block/header progress observation window (default: 30)
  --block-progress-timeout <sec>   Max wait for first block-height increment (default: 180)
  --allow-non-terminal-qv          Set NEO_TEE_SGX_ALLOW_NON_TERMINAL_QV=1 for this run
  --allow-expired-collateral       Set NEO_TEE_SGX_ALLOW_EXPIRED_COLLATERAL=1 for this run
  --no-generate-evidence           Do not auto-generate evidence if files are missing
  --require-block-progress         Fail if block height does not increase during sync window
  -h, --help                       Show this help
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
  --config)
    CONFIG_PATH="$2"
    shift 2
    ;;
  --binary)
    NODE_BIN="$2"
    shift 2
    ;;
  --tee-data-path)
    TEE_DATA_PATH="$2"
    shift 2
    ;;
  --storage)
    STORAGE_PATH="$2"
    shift 2
    ;;
  --ordering-policy)
    ORDERING_POLICY="$2"
    shift 2
    ;;
  --listen-port)
    LISTEN_PORT="$2"
    shift 2
    ;;
  --rpc-port)
    RPC_PORT="$2"
    shift 2
    ;;
  --rpc-url)
    RPC_URL="$2"
    RPC_URL_EXPLICIT=1
    shift 2
    ;;
  --rpc-user)
    RPC_USER="$2"
    shift 2
    ;;
  --rpc-pass)
    RPC_PASS="$2"
    shift 2
    ;;
  --iterations)
    ITERATIONS="$2"
    shift 2
    ;;
  --startup-timeout)
    STARTUP_TIMEOUT="$2"
    shift 2
    ;;
  --sync-window)
    SYNC_WINDOW="$2"
    shift 2
    ;;
  --block-progress-timeout)
    BLOCK_PROGRESS_TIMEOUT="$2"
    shift 2
    ;;
  --allow-non-terminal-qv)
    ALLOW_NON_TERMINAL_QV=1
    shift
    ;;
  --allow-expired-collateral)
    ALLOW_EXPIRED_COLLATERAL=1
    shift
    ;;
  --no-generate-evidence)
    GENERATE_EVIDENCE=0
    shift
    ;;
  --require-block-progress)
    REQUIRE_BLOCK_PROGRESS=1
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

if [[ -n "$RPC_PORT" ]]; then
  rpc_url_from_port="http://127.0.0.1:${RPC_PORT}"
  if [[ "$RPC_URL_EXPLICIT" -eq 0 ]]; then
    RPC_URL="$rpc_url_from_port"
  elif [[ "$RPC_URL" != "$rpc_url_from_port" ]]; then
    echo "warning: --rpc-url ($RPC_URL) does not match --rpc-port ($RPC_PORT); using explicit RPC URL" >&2
  fi
fi

if ! command -v jq >/dev/null 2>&1; then
  echo "error: jq is required" >&2
  exit 1
fi

if [[ ! -x "$NODE_BIN" ]]; then
  echo "error: neo-node binary not found or not executable: $NODE_BIN" >&2
  exit 1
fi

if ! "$NODE_BIN" --help 2>/dev/null | grep -q -- '--tee'; then
  cat >&2 <<EOF
error: neo-node binary does not expose --tee flag: $NODE_BIN
hint: build with tee-sgx feature, e.g.
  cargo build -p neo-node --features tee-sgx
or pass --binary <path> to a tee-enabled neo-node binary.
EOF
  exit 1
fi

mkdir -p "$TEE_DATA_PATH"

if [[ ! -s "$TEE_DATA_PATH/sgx.quote" || ! -s "$TEE_DATA_PATH/sgx.sealing_key" ]]; then
  if [[ "$GENERATE_EVIDENCE" -eq 1 ]]; then
    echo "SGX evidence missing; generating fresh quote+sealing-key into $TEE_DATA_PATH"
    ./scripts/generate-sgx-evidence.sh "$TEE_DATA_PATH"
  else
    echo "error: SGX evidence missing in $TEE_DATA_PATH and --no-generate-evidence set" >&2
    exit 1
  fi
fi

rpc_call() {
  local method="$1"
  local params="${2:-[]}"
  curl --compressed -sS \
    -u "${RPC_USER}:${RPC_PASS}" \
    -H 'content-type: application/json' \
    -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"${method}\",\"params\":${params}}" \
    "$RPC_URL"
}

LOG_FILE="$(mktemp /tmp/neo-tee-sgx-validate-XXXXXX.log)"
NODE_PID=""

cleanup() {
  if [[ -n "$NODE_PID" ]] && kill -0 "$NODE_PID" >/dev/null 2>&1; then
    kill -INT "$NODE_PID" >/dev/null 2>&1 || true
    wait "$NODE_PID" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

fail() {
  echo "error: $*" >&2
  if [[ -f "$LOG_FILE" ]]; then
    echo "--- neo-node log tail ($LOG_FILE) ---" >&2
    tail -n 120 "$LOG_FILE" >&2 || true
    if grep -q "configuration and software hardening needed" "$LOG_FILE"; then
      cat >&2 <<'EOF'
hint: DCAP returned a non-terminal QV status (e.g. 0xA008).
      Remediate platform microcode/TCB/collateral first; if you need to proceed temporarily, rerun with:
        --allow-non-terminal-qv
EOF
    fi
    if grep -q "SGX collateral expired" "$LOG_FILE"; then
      cat >&2 <<'EOF'
hint: SGX collateral is expired.
      Refresh quote collateral first; if you need to proceed temporarily, rerun with:
        --allow-expired-collateral
EOF
    fi
    if grep -q "failed to bind TCP listener" "$LOG_FILE"; then
      cat >&2 <<'EOF'
hint: local listen port is already in use.
      Rerun with a free listen/RPC port pair, for example:
        --listen-port 30333 --rpc-port 30332 --rpc-url http://127.0.0.1:30332
EOF
    fi
  fi
  exit 1
}

NODE_ARGS=(
  "$NODE_BIN"
  --config "$CONFIG_PATH"
  --tee
  --tee-data-path "$TEE_DATA_PATH"
  --tee-ordering-policy "$ORDERING_POLICY"
)
if [[ -n "$STORAGE_PATH" ]]; then
  NODE_ARGS+=(--storage "$STORAGE_PATH")
fi
if [[ -n "$LISTEN_PORT" ]]; then
  NODE_ARGS+=(--listen-port "$LISTEN_PORT")
fi
if [[ -n "$RPC_PORT" ]]; then
  NODE_ARGS+=(--rpc-port "$RPC_PORT")
fi

echo "Starting neo-node in strict tee-sgx mode..."
if [[ "$ALLOW_NON_TERMINAL_QV" -eq 1 && "$ALLOW_EXPIRED_COLLATERAL" -eq 1 ]]; then
  NEO_TEE_SGX_ALLOW_NON_TERMINAL_QV=1 \
    NEO_TEE_SGX_ALLOW_EXPIRED_COLLATERAL=1 \
    "${NODE_ARGS[@]}" >"$LOG_FILE" 2>&1 &
elif [[ "$ALLOW_NON_TERMINAL_QV" -eq 1 ]]; then
  NEO_TEE_SGX_ALLOW_NON_TERMINAL_QV=1 \
    "${NODE_ARGS[@]}" >"$LOG_FILE" 2>&1 &
elif [[ "$ALLOW_EXPIRED_COLLATERAL" -eq 1 ]]; then
  NEO_TEE_SGX_ALLOW_EXPIRED_COLLATERAL=1 \
    "${NODE_ARGS[@]}" >"$LOG_FILE" 2>&1 &
else
  "${NODE_ARGS[@]}" >"$LOG_FILE" 2>&1 &
fi
NODE_PID=$!

echo "Waiting for RPC readiness at $RPC_URL (timeout: ${STARTUP_TIMEOUT}s)..."
deadline=$((SECONDS + STARTUP_TIMEOUT))
while ((SECONDS < deadline)); do
  if ! kill -0 "$NODE_PID" >/dev/null 2>&1; then
    fail "neo-node exited before RPC became ready"
  fi

  response="$(rpc_call getversion 2>/dev/null || true)"
  if jq -e '.result.protocol.network' >/dev/null 2>&1 <<<"$response"; then
    break
  fi
  sleep 1
done
if ((SECONDS >= deadline)); then
  fail "RPC did not become ready within ${STARTUP_TIMEOUT}s"
fi

required_logs=(
  "verified SGX quote and sealing key binding in strict mode"
  "TEE enclave initialized in SGX hardware mode"
  "TEE startup self-checks passed"
  "TEE wallet adapter enabled for signing"
)
for pattern in "${required_logs[@]}"; do
  if ! grep -q "$pattern" "$LOG_FILE"; then
    fail "missing required SGX/TEE log line: $pattern"
  fi
done

version_json="$(rpc_call getversion)"
network_magic="$(jq -r '.result.protocol.network // -1' <<<"$version_json")"
if [[ "$network_magic" == "-1" ]]; then
  fail "getversion did not return protocol.network"
fi

addresses_json="$(rpc_call listaddress)"
address="$(jq -r '.result[0].address // ""' <<<"$addresses_json")"
haskey="$(jq -r '.result[0].haskey // false' <<<"$addresses_json")"
if [[ -z "$address" || "$haskey" != "true" ]]; then
  fail "TEE wallet did not expose a signing account"
fi

dump_json="$(rpc_call dumpprivkey "[\"${address}\"]")"
dump_text="$(jq -r '(.error.message // "") + " " + (.error.data // "")' <<<"$dump_json")"
if [[ "$dump_text" != *"TEE accounts cannot export WIF"* ]]; then
  fail "dumpprivkey did not enforce TEE key export denial"
fi

invoke_state=""
invoke_json=""
invoke_deadline=$((SECONDS + 30))
while ((SECONDS < invoke_deadline)); do
  invoke_json="$(rpc_call invokefunction '["0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5","totalSupply",[]]')"
  invoke_state="$(jq -r '.result.state // ""' <<<"$invoke_json")"
  if [[ "$invoke_state" == "HALT" ]]; then
    break
  fi
  sleep 1
done
if [[ "$invoke_state" != "HALT" ]]; then
  fail "invokefunction totalSupply did not return HALT after warmup: $invoke_json"
fi

peer_deadline=$((SECONDS + 45))
conn=0
while ((SECONDS < peer_deadline)); do
  conn="$(rpc_call getconnectioncount | jq -r '.result // 0')"
  if ((conn > 0)); then
    break
  fi
  sleep 1
done
if ((conn < 1)); then
  fail "node did not establish peers within peer warmup window"
fi

last_header=-1
for ((i = 1; i <= ITERATIONS; i++)); do
  conn="$(rpc_call getconnectioncount | jq -r '.result // -1')"
  block="$(rpc_call getblockcount | jq -r '.result // -1')"
  header="$(rpc_call getblockheadercount | jq -r '.result // -1')"
  if ((conn < 1)); then
    sleep 1
    conn="$(rpc_call getconnectioncount | jq -r '.result // -1')"
    if ((conn < 1)); then
      fail "iteration $i: getconnectioncount returned $conn"
    fi
  fi
  if ((block < 1)); then
    fail "iteration $i: getblockcount returned $block"
  fi
  if ((header < block)); then
    fail "iteration $i: header height $header is below block height $block"
  fi
  if ((last_header > header)); then
    fail "iteration $i: header height regressed from $last_header to $header"
  fi

  if ((i == 1 || i % 10 == 0)); then
    inv_state="$(rpc_call invokefunction '["0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5","totalSupply",[]]' | jq -r '.result.state // ""')"
    if [[ "$inv_state" != "HALT" ]]; then
      fail "iteration $i: invokefunction totalSupply state=$inv_state"
    fi
    dump_text="$(rpc_call dumpprivkey "[\"${address}\"]" | jq -r '(.error.message // "") + " " + (.error.data // "")')"
    if [[ "$dump_text" != *"TEE accounts cannot export WIF"* ]]; then
      fail "iteration $i: dumpprivkey unexpectedly allowed export"
    fi
  fi

  last_header="$header"
  sleep 0.1
done

block_before="$(rpc_call getblockcount | jq -r '.result // -1')"
header_before="$(rpc_call getblockheadercount | jq -r '.result // -1')"
sleep "$SYNC_WINDOW"
block_after="$(rpc_call getblockcount | jq -r '.result // -1')"
header_after="$(rpc_call getblockheadercount | jq -r '.result // -1')"
block_delta=$((block_after - block_before))
header_delta=$((header_after - header_before))

if ((REQUIRE_BLOCK_PROGRESS == 1 && block_delta <= 0)); then
  progress_deadline=$((SECONDS + BLOCK_PROGRESS_TIMEOUT))
  while ((SECONDS < progress_deadline)); do
    block_after="$(rpc_call getblockcount | jq -r '.result // -1')"
    header_after="$(rpc_call getblockheadercount | jq -r '.result // -1')"
    block_delta=$((block_after - block_before))
    header_delta=$((header_after - header_before))
    if ((block_delta > 0)); then
      break
    fi
    sleep 1
  done
  if ((block_delta <= 0)); then
    fail "block height did not advance within ${BLOCK_PROGRESS_TIMEOUT}s after initial ${SYNC_WINDOW}s sync window"
  fi
fi

if ((block_delta <= 0)); then
  echo "warning: block height did not advance during ${SYNC_WINDOW}s (delta=$block_delta)"
fi

if ((header_delta < 0)); then
  fail "header height regressed during sync window"
fi

echo
echo "TEE-SGX validation succeeded"
echo "  network_magic:       $network_magic"
echo "  iterations_passed:   $ITERATIONS"
echo "  block_height:        $block_after (delta: $block_delta)"
echo "  header_height:       $header_after (delta: $header_delta)"
echo "  log_file:            $LOG_FILE"
