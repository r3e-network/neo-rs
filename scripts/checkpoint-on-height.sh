#!/usr/bin/env bash
set -euo pipefail

# Hardlink-snapshot the live mainnet chain DB + StateRoot DB every <interval>
# blocks. Polls block height via JSON-RPC (default http://localhost:10332/).
#
# Snapshots are stored as:
#   <checkpoint_root>/h<height>/mainnet/
#   <checkpoint_root>/h<height>/StateRoot/
#
# Hardlink copies cost ~zero disk for SST files (immutable, shared inodes) and
# negligible time, so the writer pause window is sub-second. Hardlinks require
# the checkpoint dir to live on the same filesystem as the data dirs.
#
# Usage:
#   scripts/checkpoint-on-height.sh <writer_pid|none> [options]
#
# Options (all have env-var fallbacks):
#   --interval <N>          checkpoint every N blocks (default 50000)
#   --max <K>               retain at most K checkpoints (default 10)
#   --rpc <url>             RPC endpoint (default http://localhost:10332/)
#   --data-dir <path>       neo-rs data root (default ./data)
#   --root <path>           checkpoint root (default <data-dir>/checkpoints)
#   --once                  take a single checkpoint at current height and exit
#   --height <N>            override RPC; use N as the height label (for --once)
#
# Environment overrides:
#   NEO_CHECKPOINT_INTERVAL, NEO_CHECKPOINT_MAX, NEO_RPC_URL,
#   NEO_DATA_DIR, NEO_CHECKPOINT_ROOT
#
# Pass `none` (or 0) as writer_pid when no live writer is running — STOP/CONT
# is skipped. Otherwise the writer is paused around each hardlink copy.

WRITER_PID="${1:-}"
shift || true

INTERVAL_BLOCKS="${NEO_CHECKPOINT_INTERVAL:-50000}"
MAX_CHECKPOINTS="${NEO_CHECKPOINT_MAX:-10}"
RPC_URL="${NEO_RPC_URL:-http://localhost:10332/}"
DATA_DIR="${NEO_DATA_DIR:-./data}"
CHECKPOINT_ROOT="${NEO_CHECKPOINT_ROOT:-}"
ONCE=0
HEIGHT_OVERRIDE=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --interval) INTERVAL_BLOCKS="$2"; shift 2;;
    --max)      MAX_CHECKPOINTS="$2"; shift 2;;
    --rpc)      RPC_URL="$2"; shift 2;;
    --data-dir) DATA_DIR="$2"; shift 2;;
    --root)     CHECKPOINT_ROOT="$2"; shift 2;;
    --once)     ONCE=1; shift;;
    --height)   HEIGHT_OVERRIDE="$2"; shift 2;;
    -h|--help)
      sed -n '3,29p' "$0"; exit 0;;
    *) echo "unknown option: $1" >&2; exit 1;;
  esac
done

if [[ -z "$WRITER_PID" ]]; then
  echo "Usage: $0 <writer_pid|none> [--interval N] [--max K] [--rpc URL] [--data-dir PATH] [--root PATH] [--once]" >&2
  exit 1
fi

CHAIN_DB="${DATA_DIR}/mainnet"
STATEROOT_DB="${DATA_DIR}/Plugins/mainnet/StateRoot"
CHECKPOINT_ROOT="${CHECKPOINT_ROOT:-${DATA_DIR}/checkpoints}"

if [[ ! -d "$CHAIN_DB" ]]; then
  echo "chain DB not found: $CHAIN_DB" >&2; exit 1
fi
if [[ ! -d "$STATEROOT_DB" ]]; then
  echo "StateRoot DB not found: $STATEROOT_DB" >&2; exit 1
fi

case "$INTERVAL_BLOCKS" in ''|*[!0-9]*) echo "--interval must be integer"; exit 1;; esac
case "$MAX_CHECKPOINTS" in ''|*[!0-9]*) echo "--max must be integer"; exit 1;; esac
if [[ "$INTERVAL_BLOCKS" -lt 1 ]]; then echo "--interval must be >=1"; exit 1; fi
if [[ "$MAX_CHECKPOINTS" -lt 1 ]]; then echo "--max must be >=1"; exit 1; fi

mkdir -p "$CHECKPOINT_ROOT"

# Cross-filesystem hardlinks fail. Detect once.
DATA_FS=$(stat -c %m "$DATA_DIR" 2>/dev/null || echo /)
CKPT_FS=$(stat -c %m "$CHECKPOINT_ROOT" 2>/dev/null || echo /)
if [[ "$DATA_FS" != "$CKPT_FS" ]]; then
  echo "WARN: $CHECKPOINT_ROOT ($CKPT_FS) and $DATA_DIR ($DATA_FS) are on different filesystems." >&2
  echo "      Hardlinks will fail and full copies (slow, disk-heavy) will be used instead." >&2
fi

has_writer() {
  [[ "$WRITER_PID" != "none" && "$WRITER_PID" != "0" ]]
}

if has_writer; then
  if ! [[ "$WRITER_PID" =~ ^[0-9]+$ ]]; then
    echo "writer_pid must be numeric or 'none': $WRITER_PID" >&2; exit 1
  fi
  if ! kill -0 "$WRITER_PID" 2>/dev/null; then
    echo "writer process not running: pid=$WRITER_PID" >&2; exit 1
  fi
fi

# ----- RPC height polling -----
fetch_height() {
  # getblockcount returns count = height + 1 ; we want height (= count - 1).
  local resp count
  resp=$(curl -s -m 3 -X POST -H 'Content-Type: application/json' \
        --data '{"jsonrpc":"2.0","id":1,"method":"getblockcount","params":[]}' \
        "$RPC_URL" 2>/dev/null) || return 1
  count=$(printf '%s' "$resp" | sed -n 's/.*"result"[[:space:]]*:[[:space:]]*\([0-9]\+\).*/\1/p')
  if [[ -z "$count" ]]; then return 1; fi
  printf '%s\n' $((count - 1))
}

# ----- snapshot operation -----
take_snapshot() {
  local height="$1"
  local target="${CHECKPOINT_ROOT}/h${height}"
  local tmp="${target}.partial"

  if [[ -d "$target" ]]; then
    echo "checkpoint h${height} already exists, skipping"; return 0
  fi

  rm -rf "$tmp"; mkdir -p "$tmp"
  echo "started_at=$(date -Iseconds)" >"${tmp}/CHECKPOINT_IN_PROGRESS"
  echo "height=${height}" >>"${tmp}/CHECKPOINT_IN_PROGRESS"
  echo "writer_pid=${WRITER_PID}" >>"${tmp}/CHECKPOINT_IN_PROGRESS"

  local paused=0
  cleanup_pause() { [[ "$paused" -eq 1 ]] && kill -CONT "$WRITER_PID" 2>/dev/null || true; }
  trap cleanup_pause RETURN

  if has_writer; then
    kill -STOP "$WRITER_PID"; paused=1
    # Allow in-flight syscalls (esp. write()) to settle before hardlinking
    sleep 0.2
  fi

  # cp -al = archive + hardlink. Falls back to copy if cross-FS.
  cp -al "$CHAIN_DB" "${tmp}/mainnet" 2>/dev/null || cp -a "$CHAIN_DB" "${tmp}/mainnet"
  cp -al "$STATEROOT_DB" "${tmp}/StateRoot" 2>/dev/null || cp -a "$STATEROOT_DB" "${tmp}/StateRoot"

  if has_writer; then
    kill -CONT "$WRITER_PID"; paused=0
  fi
  trap - RETURN

  {
    echo "completed_at=$(date -Iseconds)"
    echo "height=${height}"
    echo "writer_pid=${WRITER_PID}"
    echo "chain_db=${CHAIN_DB}"
    echo "stateroot_db=${STATEROOT_DB}"
  } >"${tmp}/CHECKPOINT_INFO"
  rm -f "${tmp}/CHECKPOINT_IN_PROGRESS"
  mv "$tmp" "$target"
  echo "checkpoint h${height} -> ${target}"
}

prune_old() {
  local -a dirs
  mapfile -t dirs < <(ls -1d "${CHECKPOINT_ROOT}"/h[0-9]* 2>/dev/null | sort -t h -k2,2n)
  local count="${#dirs[@]}"
  if [[ "$count" -le "$MAX_CHECKPOINTS" ]]; then return; fi
  local to_prune=$((count - MAX_CHECKPOINTS))
  local i
  for ((i = 0; i < to_prune; i++)); do
    echo "pruning old checkpoint: ${dirs[$i]}"
    rm -rf "${dirs[$i]}"
  done
}

# ----- main loop -----
if [[ "$ONCE" -eq 1 ]]; then
  if [[ -n "$HEIGHT_OVERRIDE" ]]; then
    case "$HEIGHT_OVERRIDE" in ''|*[!0-9]*) echo "--height must be integer"; exit 1;; esac
    H="$HEIGHT_OVERRIDE"
  else
    H=$(fetch_height) || { echo "RPC unreachable at $RPC_URL (use --height N to override)" >&2; exit 1; }
  fi
  take_snapshot "$H"
  prune_old
  exit 0
fi

if ! has_writer; then
  echo "writer_pid=none requires --once (no looping without a live writer)" >&2
  exit 1
fi

echo "watching pid=$WRITER_PID interval=${INTERVAL_BLOCKS} blocks max=$MAX_CHECKPOINTS rpc=$RPC_URL root=$CHECKPOINT_ROOT"
last_height=-1
# seed last_height from highest existing checkpoint so we don't re-checkpoint h<X> immediately after restart
if last_ckpt=$(ls -1d "${CHECKPOINT_ROOT}"/h[0-9]* 2>/dev/null | sort -t h -k2,2n | tail -1); then
  if [[ -n "$last_ckpt" ]]; then
    last_height=$(basename "$last_ckpt" | sed 's/^h//')
    echo "resuming after last checkpoint h${last_height}"
  fi
fi

while kill -0 "$WRITER_PID" 2>/dev/null; do
  if H=$(fetch_height); then
    target=$(( (last_height < 0 ? 0 : (last_height / INTERVAL_BLOCKS + 1) * INTERVAL_BLOCKS) ))
    if (( H >= target )); then
      # Snap at the highest multiple-of-interval that is <= H, to keep heights tidy
      snap_height=$(( (H / INTERVAL_BLOCKS) * INTERVAL_BLOCKS ))
      if (( snap_height > last_height )); then
        take_snapshot "$snap_height"
        prune_old
        last_height=$snap_height
      fi
    fi
  else
    echo "RPC poll failed; will retry"
  fi
  sleep 15
done

echo "writer pid=$WRITER_PID exited; checkpoint loop stopping"
