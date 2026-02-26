#!/usr/bin/env bash
set -euo pipefail

# Periodic checkpoint loop for a live RocksDB writer process.
#
# Usage:
#   ./scripts/checkpoint-live-rocksdb-loop.sh <writer_pid> <rocksdb_path> [interval_secs] [max_checkpoints] [checkpoint_root]
#
# Example:
#   ./scripts/checkpoint-live-rocksdb-loop.sh 12345 data/testnet-pre128955 1800 8 checkpoints

if [[ $# -lt 2 || $# -gt 5 ]]; then
  echo "Usage: $0 <writer_pid> <rocksdb_path> [interval_secs] [max_checkpoints] [checkpoint_root]" >&2
  exit 1
fi

WRITER_PID="$1"
DB_PATH="$2"
INTERVAL_SECS="${3:-1800}"
MAX_CHECKPOINTS="${4:-8}"
CHECKPOINT_ROOT="${5:-checkpoints}"

if ! [[ "$WRITER_PID" =~ ^[0-9]+$ ]]; then
  echo "writer_pid must be numeric: $WRITER_PID" >&2
  exit 1
fi

if ! [[ "$INTERVAL_SECS" =~ ^[0-9]+$ ]] || [[ "$INTERVAL_SECS" -lt 10 ]]; then
  echo "interval_secs must be an integer >= 10: $INTERVAL_SECS" >&2
  exit 1
fi

if ! [[ "$MAX_CHECKPOINTS" =~ ^[0-9]+$ ]] || [[ "$MAX_CHECKPOINTS" -lt 1 ]]; then
  echo "max_checkpoints must be an integer >= 1: $MAX_CHECKPOINTS" >&2
  exit 1
fi

if [[ ! -d "$DB_PATH" ]]; then
  echo "RocksDB path not found: $DB_PATH" >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CHECKPOINT_SCRIPT="${SCRIPT_DIR}/checkpoint-live-rocksdb.sh"
if [[ ! -x "$CHECKPOINT_SCRIPT" ]]; then
  echo "checkpoint helper is missing or not executable: $CHECKPOINT_SCRIPT" >&2
  exit 1
fi

DB_BASENAME="$(basename "$DB_PATH")"
mkdir -p "$CHECKPOINT_ROOT"

prune_old_checkpoints() {
  local -a checkpoints
  mapfile -t checkpoints < <(
    ls -1dt "${CHECKPOINT_ROOT}/${DB_BASENAME}"-checkpoint-* 2>/dev/null || true
  )

  local count="${#checkpoints[@]}"
  if [[ "$count" -le "$MAX_CHECKPOINTS" ]]; then
    return
  fi

  local prune_count=$((count - MAX_CHECKPOINTS))
  local i
  for ((i = count - 1; i >= count - prune_count; i--)); do
    echo "Pruning old checkpoint: ${checkpoints[$i]}"
    rm -rf -- "${checkpoints[$i]}"
  done
}

echo "Starting checkpoint loop: pid=$WRITER_PID db=$DB_PATH interval=${INTERVAL_SECS}s retain=$MAX_CHECKPOINTS root=$CHECKPOINT_ROOT"
while kill -0 "$WRITER_PID" 2>/dev/null; do
  "${CHECKPOINT_SCRIPT}" "$WRITER_PID" "$DB_PATH" "$CHECKPOINT_ROOT"
  prune_old_checkpoints

  slept=0
  while [[ "$slept" -lt "$INTERVAL_SECS" ]]; do
    if ! kill -0 "$WRITER_PID" 2>/dev/null; then
      echo "Writer process exited: pid=$WRITER_PID"
      exit 0
    fi
    sleep 1
    slept=$((slept + 1))
  done
done

echo "Writer process is not running: pid=$WRITER_PID"
