#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  scripts/verify-testnet-storage-parity.sh \
    <storage_path> <height> <script_hash> <contract_id> <key_suffix_hex> [rpc_url]

Example (GAS totalSupply key at height 128955):
  scripts/verify-testnet-storage-parity.sh \
    data/repro-diverge-fixed-run 128955 \
    0xd2a4cff31913016155e38e474a2c06d08be276cf \
    -6 0b http://seed1t5.neo.org:20332

This script verifies:
1) local block hash == remote getblockhash(height)
2) local storage value == remote getstate(getstateroot(height).roothash, script_hash, key)

Safety:
- Run this script only against a quiesced DB (no active writer on the same `--storage` path).
- Override the live-writer guard only for debugging with `NEO_VERIFY_ALLOW_LIVE_DB=1`.
USAGE
}

if [[ $# -lt 5 || $# -gt 6 ]]; then
  usage >&2
  exit 1
fi

for cmd in cargo curl jq xxd base64; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "required command not found: $cmd" >&2
    exit 1
  fi
done

STORAGE_PATH="$1"
HEIGHT="$2"
SCRIPT_HASH="$3"
CONTRACT_ID="$4"
KEY_SUFFIX_HEX_RAW="$5"
RPC_URL="${6:-http://seed1t5.neo.org:20332}"

if [[ ! -d "$STORAGE_PATH" ]]; then
  echo "storage_path not found: $STORAGE_PATH" >&2
  exit 1
fi

canonical_path() {
  local path="$1"
  if command -v realpath >/dev/null 2>&1; then
    realpath "$path"
  elif command -v readlink >/dev/null 2>&1; then
    readlink -f "$path" 2>/dev/null || printf '%s\n' "$path"
  else
    printf '%s\n' "$path"
  fi
}

STORAGE_PATH_CANON="$(canonical_path "$STORAGE_PATH")"

if [[ "${NEO_VERIFY_ALLOW_LIVE_DB:-0}" != "1" ]]; then
  LIVE_WRITER_PID="$(
    ps -eo pid=,args= | awk -v raw="$STORAGE_PATH" -v canon="$STORAGE_PATH_CANON" '
      /neo-node/ && (index($0, "--storage " raw) || index($0, "--storage=" raw) || index($0, "--storage " canon) || index($0, "--storage=" canon)) {
        print $1
        exit
      }
    '
  )"
  if [[ -n "$LIVE_WRITER_PID" ]]; then
    echo "detected active neo-node writer on this storage path (pid=$LIVE_WRITER_PID)." >&2
    echo "stop the writer before parity checks, or set NEO_VERIFY_ALLOW_LIVE_DB=1 to override." >&2
    exit 1
  fi
fi

if ! [[ "$HEIGHT" =~ ^[0-9]+$ ]]; then
  echo "height must be a non-negative integer: $HEIGHT" >&2
  exit 1
fi

normalize_hex() {
  local raw="$1"
  local stripped="${raw#0x}"
  stripped="${stripped#0X}"
  printf '%s' "$stripped" | tr '[:upper:]' '[:lower:]'
}

KEY_SUFFIX_HEX="$(normalize_hex "$KEY_SUFFIX_HEX_RAW")"
if [[ -n "$KEY_SUFFIX_HEX" ]]; then
  if (( ${#KEY_SUFFIX_HEX} % 2 != 0 )); then
    echo "key_suffix_hex must have even hex length: $KEY_SUFFIX_HEX_RAW" >&2
    exit 1
  fi
  if ! [[ "$KEY_SUFFIX_HEX" =~ ^[0-9a-f]+$ ]]; then
    echo "key_suffix_hex must be valid hex: $KEY_SUFFIX_HEX_RAW" >&2
    exit 1
  fi
fi

rpc_call() {
  local method="$1"
  local params_json="$2"
  curl --compressed -sS -m 30 \
    -H 'content-type: application/json' \
    -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"${method}\",\"params\":${params_json}}" \
    "$RPC_URL"
}

LOCAL_HEIGHT_OUT="$(
  cargo run -q -p neo-core --features rocksdb --example print_height -- "$STORAGE_PATH" "$HEIGHT" \
    | tr -d '\r'
)"

LOCAL_HEIGHT="$(printf '%s\n' "$LOCAL_HEIGHT_OUT" | awk -F= '/^current_index=/{print $2; exit}')"
LOCAL_HASH="$(printf '%s\n' "$LOCAL_HEIGHT_OUT" | awk -F= -v h="$HEIGHT" '$1=="block_hash[" h "]"{print $2; exit}')"

if [[ -z "$LOCAL_HEIGHT" || -z "$LOCAL_HASH" || "$LOCAL_HASH" == "<none>" ]]; then
  echo "failed to read local height/hash from print_height output:" >&2
  printf '%s\n' "$LOCAL_HEIGHT_OUT" >&2
  exit 1
fi

if (( LOCAL_HEIGHT < HEIGHT )); then
  echo "local height ($LOCAL_HEIGHT) is below requested height ($HEIGHT)" >&2
  exit 1
fi

if (( LOCAL_HEIGHT != HEIGHT )); then
  echo "state parity check requires height == local tip (requested=$HEIGHT local_tip=$LOCAL_HEIGHT)" >&2
  echo "use print_height/getblockhash for historical hash parity; storage-key parity is only valid at current local tip" >&2
  exit 1
fi

REMOTE_BLOCK_RESP="$(rpc_call "getblockhash" "[${HEIGHT}]")"
REMOTE_BLOCK_HASH="$(printf '%s\n' "$REMOTE_BLOCK_RESP" | jq -r '.result // empty')"
if [[ -z "$REMOTE_BLOCK_HASH" ]]; then
  echo "failed to fetch remote block hash at height=$HEIGHT from $RPC_URL" >&2
  printf '%s\n' "$REMOTE_BLOCK_RESP" >&2
  exit 1
fi

REMOTE_ROOT_RESP="$(rpc_call "getstateroot" "[${HEIGHT}]")"
REMOTE_ROOT_HASH="$(printf '%s\n' "$REMOTE_ROOT_RESP" | jq -r '.result.roothash // empty')"
if [[ -z "$REMOTE_ROOT_HASH" ]]; then
  echo "failed to fetch remote state root at height=$HEIGHT from $RPC_URL" >&2
  printf '%s\n' "$REMOTE_ROOT_RESP" >&2
  exit 1
fi

KEY_SUFFIX_B64="$(printf '%s' "$KEY_SUFFIX_HEX" | xxd -r -p | base64 | tr -d '\n')"
REMOTE_STATE_RESP="$(rpc_call "getstate" "[\"${REMOTE_ROOT_HASH}\",\"${SCRIPT_HASH}\",\"${KEY_SUFFIX_B64}\"]")"
REMOTE_STATE_ERR="$(printf '%s\n' "$REMOTE_STATE_RESP" | jq -r '.error.message // empty')"
REMOTE_STATE_ERR_CODE="$(printf '%s\n' "$REMOTE_STATE_RESP" | jq -r '.error.code // empty')"
if [[ -n "$REMOTE_STATE_ERR" ]]; then
  case "$REMOTE_STATE_ERR" in
    *"not present in the dictionary"*)
      REMOTE_STATE_B64=""
      ;;
    *)
      if [[ "$REMOTE_STATE_ERR_CODE" == "-2146232969" ]]; then
        REMOTE_STATE_B64=""
      else
        echo "remote getstate returned error: $REMOTE_STATE_ERR" >&2
        printf '%s\n' "$REMOTE_STATE_RESP" >&2
        exit 1
      fi
      ;;
  esac
else
  REMOTE_STATE_B64="$(printf '%s\n' "$REMOTE_STATE_RESP" | jq -r '.result // empty')"
fi

LOCAL_STATE_OUT="$(
  cargo run -q -p neo-core --features rocksdb --example print_storage_key -- \
    "$STORAGE_PATH" "$CONTRACT_ID" "$KEY_SUFFIX_HEX" \
    | tr -d '\r'
)"
LOCAL_VALUE_HEX="$(printf '%s\n' "$LOCAL_STATE_OUT" | awk -F= '/^value_hex=/{print $2; exit}')"
if [[ -z "$LOCAL_VALUE_HEX" ]]; then
  echo "failed to parse local storage output:" >&2
  printf '%s\n' "$LOCAL_STATE_OUT" >&2
  exit 1
fi

if [[ "$LOCAL_VALUE_HEX" == "<none>" ]]; then
  LOCAL_STATE_B64=""
else
  LOCAL_VALUE_HEX="${LOCAL_VALUE_HEX#0x}"
  LOCAL_STATE_B64="$(printf '%s' "$LOCAL_VALUE_HEX" | xxd -r -p | base64 | tr -d '\n')"
fi

echo "storage_path=$STORAGE_PATH"
echo "target_height=$HEIGHT"
echo "local_height=$LOCAL_HEIGHT"
echo "local_hash=$LOCAL_HASH"
echo "remote_hash=$REMOTE_BLOCK_HASH"
echo "remote_roothash=$REMOTE_ROOT_HASH"
echo "script_hash=$SCRIPT_HASH"
echo "contract_id=$CONTRACT_ID"
echo "key_suffix_hex=0x$KEY_SUFFIX_HEX"
echo "key_suffix_base64=$KEY_SUFFIX_B64"
echo "local_state_base64=${LOCAL_STATE_B64:-<none>}"
echo "remote_state_base64=${REMOTE_STATE_B64:-<none>}"

if [[ "$LOCAL_HASH" != "$REMOTE_BLOCK_HASH" ]]; then
  echo "HASH_MISMATCH at height $HEIGHT" >&2
  exit 2
fi

if [[ "$LOCAL_STATE_B64" != "$REMOTE_STATE_B64" ]]; then
  echo "STATE_MISMATCH at height $HEIGHT" >&2
  exit 3
fi

echo "parity_ok=1"
