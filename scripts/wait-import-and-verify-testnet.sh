#!/usr/bin/env bash
set -euo pipefail

# Wait for an import-only neo-node process to exit, then verify final height/hash
# against a public TestNet RPC endpoint.
#
# Usage:
#   ./scripts/wait-import-and-verify-testnet.sh <importer_pid> <storage_path> <target_height> [rpc_url]
#
# Example:
#   ./scripts/wait-import-and-verify-testnet.sh 12345 data/testnet-pre128955 497772

if [[ $# -lt 3 || $# -gt 4 ]]; then
  echo "Usage: $0 <importer_pid> <storage_path> <target_height> [rpc_url]" >&2
  exit 1
fi

IMPORTER_PID="$1"
STORAGE_PATH="$2"
TARGET_HEIGHT="$3"
RPC_URL="${4:-http://seed1t5.neo.org:20332}"

if ! [[ "$IMPORTER_PID" =~ ^[0-9]+$ ]]; then
  echo "importer_pid must be numeric: $IMPORTER_PID" >&2
  exit 1
fi

if ! [[ "$TARGET_HEIGHT" =~ ^[0-9]+$ ]]; then
  echo "target_height must be numeric: $TARGET_HEIGHT" >&2
  exit 1
fi

if [[ ! -d "$STORAGE_PATH" ]]; then
  echo "storage_path not found: $STORAGE_PATH" >&2
  exit 1
fi

echo "Waiting for importer pid=$IMPORTER_PID to exit ..."
while kill -0 "$IMPORTER_PID" 2>/dev/null; do
  sleep 20
done
echo "Importer exited. Running local/remote hash verification at height=$TARGET_HEIGHT ..."

LOCAL_OUT="$(
  cargo run -q -p neo-core --features rocksdb --example print_height -- "$STORAGE_PATH" "$TARGET_HEIGHT" \
    | tr -d '\r'
)"

LOCAL_HEIGHT="$(printf '%s\n' "$LOCAL_OUT" | awk -F= '/^current_index=/{print $2; exit}')"
LOCAL_HASH="$(printf '%s\n' "$LOCAL_OUT" | awk -F= -v h="$TARGET_HEIGHT" '$1=="block_hash[" h "]"{print $2; exit}')"

if [[ -z "$LOCAL_HEIGHT" ]]; then
  echo "failed to read local current_index from print_height output" >&2
  printf '%s\n' "$LOCAL_OUT" >&2
  exit 1
fi

if [[ "$LOCAL_HEIGHT" -lt "$TARGET_HEIGHT" ]]; then
  echo "local height below target: local=$LOCAL_HEIGHT target=$TARGET_HEIGHT" >&2
  exit 1
fi

if [[ -z "$LOCAL_HASH" || "$LOCAL_HASH" == "<none>" ]]; then
  echo "local block hash at target height missing: height=$TARGET_HEIGHT" >&2
  printf '%s\n' "$LOCAL_OUT" >&2
  exit 1
fi

REMOTE_RESP="$(
  curl --compressed -sS -m 20 \
    -H 'content-type: application/json' \
    -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getblockhash\",\"params\":[${TARGET_HEIGHT}]}" \
    "$RPC_URL"
)"
REMOTE_HASH="$(printf '%s\n' "$REMOTE_RESP" | jq -r '.result // empty')"

if [[ -z "$REMOTE_HASH" || "$REMOTE_HASH" == "null" ]]; then
  echo "failed to read remote hash at target height from $RPC_URL" >&2
  printf '%s\n' "$REMOTE_RESP" >&2
  exit 1
fi

echo "local_height=$LOCAL_HEIGHT"
echo "local_hash=$LOCAL_HASH"
echo "remote_hash=$REMOTE_HASH"
echo "target_height=$TARGET_HEIGHT"
echo "rpc_url=$RPC_URL"

if [[ "$LOCAL_HASH" != "$REMOTE_HASH" ]]; then
  echo "HASH_MISMATCH at height $TARGET_HEIGHT" >&2
  exit 2
fi

echo "hash_match=1"
