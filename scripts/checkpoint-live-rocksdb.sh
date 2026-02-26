#!/usr/bin/env bash
set -euo pipefail

# Create a point-in-time checkpoint of a live RocksDB directory by:
# 1) pausing the writer process (SIGSTOP),
# 2) copying the directory,
# 3) resuming the process (SIGCONT).
#
# Usage:
#   ./scripts/checkpoint-live-rocksdb.sh <writer_pid> <rocksdb_path> [checkpoint_root]
#
# Example:
#   ./scripts/checkpoint-live-rocksdb.sh 12345 data/testnet-pre128955 checkpoints

if [[ $# -lt 2 || $# -gt 3 ]]; then
  echo "Usage: $0 <writer_pid> <rocksdb_path> [checkpoint_root]" >&2
  exit 1
fi

WRITER_PID="$1"
DB_PATH="$2"
CHECKPOINT_ROOT="${3:-checkpoints}"

if ! [[ "$WRITER_PID" =~ ^[0-9]+$ ]]; then
  echo "writer_pid must be numeric: $WRITER_PID" >&2
  exit 1
fi

if ! kill -0 "$WRITER_PID" 2>/dev/null; then
  echo "writer process is not running: pid=$WRITER_PID" >&2
  exit 1
fi

if [[ ! -d "$DB_PATH" ]]; then
  echo "RocksDB path not found: $DB_PATH" >&2
  exit 1
fi

mkdir -p "$CHECKPOINT_ROOT"
TS="$(date +%Y%m%d-%H%M%S)"
DB_BASENAME="$(basename "$DB_PATH")"
CHECKPOINT_DIR="${CHECKPOINT_ROOT}/${DB_BASENAME}-checkpoint-${TS}"
CHECKPOINT_DONE=0

resume_writer() {
  kill -CONT "$WRITER_PID" 2>/dev/null || true
}

cleanup() {
  resume_writer
  if [[ "$CHECKPOINT_DONE" -eq 1 ]]; then
    rm -f "${CHECKPOINT_DIR}/CHECKPOINT_IN_PROGRESS" 2>/dev/null || true
  fi
}

trap cleanup EXIT INT TERM HUP

echo "Pausing writer pid=$WRITER_PID ..."
kill -STOP "$WRITER_PID"

echo "Copying $DB_PATH -> $CHECKPOINT_DIR ..."
mkdir -p "$CHECKPOINT_DIR"
echo "started_at=$(date -Iseconds)" >"${CHECKPOINT_DIR}/CHECKPOINT_IN_PROGRESS"
echo "writer_pid=${WRITER_PID}" >>"${CHECKPOINT_DIR}/CHECKPOINT_IN_PROGRESS"
echo "source_path=${DB_PATH}" >>"${CHECKPOINT_DIR}/CHECKPOINT_IN_PROGRESS"
if command -v rsync >/dev/null 2>&1; then
  rsync -a "$DB_PATH"/ "$CHECKPOINT_DIR"/
else
  cp -a "$DB_PATH"/. "$CHECKPOINT_DIR"/
fi

echo "resumed_at=$(date -Iseconds)" >"${CHECKPOINT_DIR}/CHECKPOINT_INFO"
echo "writer_pid=${WRITER_PID}" >>"${CHECKPOINT_DIR}/CHECKPOINT_INFO"
echo "source_path=${DB_PATH}" >>"${CHECKPOINT_DIR}/CHECKPOINT_INFO"
CHECKPOINT_DONE=1

echo "Checkpoint complete: $CHECKPOINT_DIR"
