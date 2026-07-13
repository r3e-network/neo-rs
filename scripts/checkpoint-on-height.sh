#!/usr/bin/env bash
set -euo pipefail

# Snapshot the live mainnet Ledger + StateService state every <interval> blocks.
# Polls block height via JSON-RPC (default http://localhost:10332/).
#
# Snapshots are stored as:
#   <checkpoint_root>/h<height>/mainnet/
#   <checkpoint_root>/h<height>/StateRoot/  (RocksDB only)
#
# RocksDB snapshots use hardlinks for immutable SST files when possible. MDBX
# snapshots must not hardlink the mutable environment files, so they use
# provider-aware copy/reflink semantics.
#
# Usage:
#   scripts/checkpoint-on-height.sh <writer_pid|none> [options]
#
# Options (all have env-var fallbacks):
#   --interval <N>          checkpoint every N blocks (default 50000)
#   --max <K>               retain at most K checkpoints (default 10)
#   --rpc <url>             RPC endpoint (default http://localhost:10332/)
#   --data-dir <path>       neo-rs data root (default ./data)
#   --chain-db <path>       explicit chain store path (default <data-dir>/mainnet)
#   --stateroot-db <path>   RocksDB StateRoot path (MDBX uses --chain-db)
#   --storage-provider <P>  storage backend encoded in the snapshot (default mdbx)
#   --chain-only            snapshot only the chain DB (for bounded replay DBs without StateService)
#   --root <path>           checkpoint root (default <data-dir>/checkpoints)
#   --once                  take a single checkpoint at current height and exit
#   --height <N>            override RPC; use N as the height label (for --once)
#   --restore-verified      mark snapshot as restore/probe verified
#   --verified-height <N>   height proven by the restore/probe verification
#   --verified-stateroot-root <HASH>
#                           StateRoot hash proven at --verified-height
#   --verified-against-reference
#                           mark snapshot as checked against reference RPC state roots
#
# Environment overrides:
#   NEO_CHECKPOINT_INTERVAL, NEO_CHECKPOINT_MAX, NEO_RPC_URL,
#   NEO_DATA_DIR, NEO_CHECKPOINT_ROOT
#
# Pass `none` (or 0) as writer_pid when no live writer is running — STOP/CONT
# is skipped. Otherwise the writer is paused around each snapshot copy.

WRITER_PID="${1:-}"
shift || true

INTERVAL_BLOCKS="${NEO_CHECKPOINT_INTERVAL:-50000}"
MAX_CHECKPOINTS="${NEO_CHECKPOINT_MAX:-10}"
RPC_URL="${NEO_RPC_URL:-http://localhost:10332/}"
DATA_DIR="${NEO_DATA_DIR:-./data}"
CHECKPOINT_ROOT="${NEO_CHECKPOINT_ROOT:-}"
CHAIN_DB_OVERRIDE="${NEO_CHAIN_DB:-}"
STATEROOT_DB_OVERRIDE="${NEO_STATEROOT_DB:-}"
STORAGE_PROVIDER="${NEO_STORAGE_PROVIDER:-mdbx}"
ONCE=0
HEIGHT_OVERRIDE=""
CHAIN_ONLY=0
RESTORE_VERIFIED=0
VERIFIED_HEIGHT=""
VERIFIED_STATEROOT_ROOT=""
VERIFIED_AGAINST_REFERENCE=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --interval) INTERVAL_BLOCKS="$2"; shift 2;;
    --max)      MAX_CHECKPOINTS="$2"; shift 2;;
    --rpc)      RPC_URL="$2"; shift 2;;
    --data-dir) DATA_DIR="$2"; shift 2;;
    --chain-db) CHAIN_DB_OVERRIDE="$2"; shift 2;;
    --stateroot-db) STATEROOT_DB_OVERRIDE="$2"; shift 2;;
    --storage-provider) STORAGE_PROVIDER="$2"; shift 2;;
    --chain-only) CHAIN_ONLY=1; shift;;
    --root)     CHECKPOINT_ROOT="$2"; shift 2;;
    --once)     ONCE=1; shift;;
    --height)   HEIGHT_OVERRIDE="$2"; shift 2;;
    --restore-verified) RESTORE_VERIFIED=1; shift;;
    --verified-height) VERIFIED_HEIGHT="$2"; shift 2;;
    --verified-stateroot-root) VERIFIED_STATEROOT_ROOT="$2"; shift 2;;
    --verified-against-reference) VERIFIED_AGAINST_REFERENCE=1; shift;;
    -h|--help)
      sed -n '3,33p' "$0"; exit 0;;
    *) echo "unknown option: $1" >&2; exit 1;;
  esac
done

if [[ -z "$WRITER_PID" ]]; then
  echo "Usage: $0 <writer_pid|none> [--interval N] [--max K] [--rpc URL] [--data-dir PATH] [--chain-db PATH] [--stateroot-db PATH] [--root PATH] [--once]" >&2
  exit 1
fi

CHAIN_DB="${CHAIN_DB_OVERRIDE:-${DATA_DIR}/mainnet}"
STATEROOT_DB="${STATEROOT_DB_OVERRIDE:-${DATA_DIR}/Plugins/mainnet/StateRoot}"
CHECKPOINT_ROOT="${CHECKPOINT_ROOT:-${DATA_DIR}/checkpoints}"
COORDINATED_MDBX=0

case "${STORAGE_PROVIDER,,}" in
  mdbx)
    COORDINATED_MDBX=1
    if [[ -n "$STATEROOT_DB_OVERRIDE" && "$STATEROOT_DB_OVERRIDE" != "$CHAIN_DB" ]]; then
      echo "MDBX StateService is stored in --chain-db; omit --stateroot-db or use the same path" >&2
      exit 1
    fi
    STATEROOT_DB="$CHAIN_DB"
    ;;
  rocksdb) ;;
  *) echo "unsupported storage provider: ${STORAGE_PROVIDER}" >&2; exit 1;;
esac

if [[ ! -d "$CHAIN_DB" ]]; then
  echo "chain DB not found: $CHAIN_DB" >&2; exit 1
fi
if [[ "$CHAIN_ONLY" -eq 0 && "$COORDINATED_MDBX" -eq 0 && ! -d "$STATEROOT_DB" ]]; then
  echo "StateRoot DB not found: $STATEROOT_DB" >&2; exit 1
fi

case "$INTERVAL_BLOCKS" in ''|*[!0-9]*) echo "--interval must be integer"; exit 1;; esac
case "$MAX_CHECKPOINTS" in ''|*[!0-9]*) echo "--max must be integer"; exit 1;; esac
if [[ -n "$VERIFIED_HEIGHT" ]]; then
  case "$VERIFIED_HEIGHT" in ''|*[!0-9]*) echo "--verified-height must be integer" >&2; exit 1;; esac
fi
if [[ "$INTERVAL_BLOCKS" -lt 1 ]]; then echo "--interval must be >=1"; exit 1; fi
if [[ "$MAX_CHECKPOINTS" -lt 3 ]]; then echo "--max must be >= 3" >&2; exit 1; fi
if [[ "$RESTORE_VERIFIED" -eq 1 && -z "$VERIFIED_HEIGHT" ]]; then
  echo "--restore-verified requires --verified-height" >&2
  exit 1
fi
if [[ "$RESTORE_VERIFIED" -eq 1 && -z "$VERIFIED_STATEROOT_ROOT" ]]; then
  echo "--restore-verified requires --verified-stateroot-root" >&2
  exit 1
fi
if [[ -n "$VERIFIED_STATEROOT_ROOT" && "$RESTORE_VERIFIED" -ne 1 ]]; then
  echo "--verified-stateroot-root requires --restore-verified" >&2
  exit 1
fi
if [[ "$VERIFIED_AGAINST_REFERENCE" -eq 1 && "$RESTORE_VERIFIED" -ne 1 ]]; then
  echo "--verified-against-reference requires --restore-verified" >&2
  exit 1
fi

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
  resp=$(curl -s --compressed -m 3 -X POST -H 'Content-Type: application/json' \
        --data '{"jsonrpc":"2.0","id":1,"method":"getblockcount","params":[]}' \
        "$RPC_URL" 2>/dev/null) || return 1
  count=$(printf '%s' "$resp" | sed -n 's/.*"result"[[:space:]]*:[[:space:]]*\([0-9]\+\).*/\1/p')
  if [[ -z "$count" ]]; then return 1; fi
  printf '%s\n' $((count - 1))
}

# ----- snapshot operation -----
copy_store_dir() {
  local src="$1"
  local dst="$2"
  case "${STORAGE_PROVIDER,,}" in
    rocksdb)
      cp -al "$src" "$dst" 2>/dev/null || cp -a "$src" "$dst"
      ;;
    mdbx)
      cp -a --reflink=auto "$src" "$dst" 2>/dev/null || cp -a "$src" "$dst"
      ;;
    *)
      echo "unsupported storage provider for checkpoint copy: ${STORAGE_PROVIDER}" >&2
      return 1
      ;;
  esac
}

take_snapshot() {
  local height="$1"
  local target="${CHECKPOINT_ROOT}/h${height}"
  local tmp="${target}.partial"

  if [[ -n "$VERIFIED_HEIGHT" && "$VERIFIED_HEIGHT" != "$height" ]]; then
    echo "--verified-height must match --height: verified=${VERIFIED_HEIGHT} height=${height}" >&2
    return 1
  fi

  if [[ -d "$target" ]]; then
    if [[ "$RESTORE_VERIFIED" -eq 1 ]]; then
      local info="${target}/CHECKPOINT_INFO"
      local existing_restore_verified existing_verified_height
      local existing_verified_stateroot_root existing_verified_against_reference
      if [[ ! -f "$info" ]]; then
        echo "checkpoint h${height} already exists but is missing CHECKPOINT_INFO" >&2
        return 1
      fi
      existing_restore_verified=$(sed -n 's/^restore_verified=//p' "$info" | head -1)
      existing_verified_height=$(sed -n 's/^verified_height=//p' "$info" | head -1)
      existing_verified_stateroot_root=$(sed -n 's/^verified_stateroot_root=//p' "$info" | head -1)
      existing_verified_against_reference=$(sed -n 's/^verified_against_reference=//p' "$info" | head -1)
      if [[ "${existing_restore_verified,,}" != "true" ]]; then
        echo "checkpoint h${height} already exists but restore_verified is not true" >&2
        return 1
      fi
      if [[ "$existing_verified_height" != "$VERIFIED_HEIGHT" ]]; then
        echo "checkpoint h${height} already exists with mismatched verified_height: existing=${existing_verified_height} requested=${VERIFIED_HEIGHT}" >&2
        return 1
      fi
      if [[ "${existing_verified_stateroot_root,,}" != "${VERIFIED_STATEROOT_ROOT,,}" ]]; then
        echo "checkpoint h${height} already exists with mismatched verified_stateroot_root: existing=${existing_verified_stateroot_root} requested=${VERIFIED_STATEROOT_ROOT}" >&2
        return 1
      fi
      if [[ "$VERIFIED_AGAINST_REFERENCE" -eq 1 && "${existing_verified_against_reference,,}" != "true" ]]; then
        echo "checkpoint h${height} already exists but verified_against_reference is not true" >&2
        return 1
      fi
    fi
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

  copy_store_dir "$CHAIN_DB" "${tmp}/mainnet"
  if [[ "$CHAIN_ONLY" -eq 0 && "$COORDINATED_MDBX" -eq 0 ]]; then
    copy_store_dir "$STATEROOT_DB" "${tmp}/StateRoot"
  fi

  if has_writer; then
    kill -CONT "$WRITER_PID"; paused=0
  fi
  trap - RETURN

  {
    echo "completed_at=$(date -Iseconds)"
    echo "height=${height}"
    echo "writer_pid=${WRITER_PID}"
    echo "chain_db=${CHAIN_DB}"
    echo "storage_provider=${STORAGE_PROVIDER}"
    if [[ "$CHAIN_ONLY" -eq 0 ]]; then
      if [[ "$COORDINATED_MDBX" -eq 1 ]]; then
        echo "stateroot_db=${CHAIN_DB}:neo_state_service"
        echo "state_root_layout=coordinated_mdbx"
      else
        echo "stateroot_db=${STATEROOT_DB}"
        echo "state_root_layout=separate"
      fi
      echo "state_root_included=true"
    else
      echo "stateroot_db=none"
      if [[ "$COORDINATED_MDBX" -eq 1 ]]; then
        echo "state_root_layout=coordinated_mdbx"
      else
        echo "state_root_layout=separate"
      fi
      echo "state_root_included=false"
    fi
    if [[ "$RESTORE_VERIFIED" -eq 1 ]]; then
      echo "restore_verified=true"
      echo "verified_height=${VERIFIED_HEIGHT}"
      if [[ -n "$VERIFIED_STATEROOT_ROOT" ]]; then
        echo "verified_stateroot_root=${VERIFIED_STATEROOT_ROOT}"
      fi
    fi
    if [[ "$VERIFIED_AGAINST_REFERENCE" -eq 1 ]]; then
      echo "verified_against_reference=true"
    fi
  } >"${tmp}/CHECKPOINT_INFO"
  rm -f "${tmp}/CHECKPOINT_IN_PROGRESS"
  mv "$tmp" "$target"
  echo "checkpoint h${height} -> ${target}"
}

prune_old() {
  local -a dirs
  dirs=()
  while IFS= read -r dir; do
    dirs+=("$dir")
  done < <(sorted_checkpoints_for_pruning)
  local count="${#dirs[@]}"
  if [[ "$count" -le "$MAX_CHECKPOINTS" ]]; then return; fi
  local to_prune=$((count - MAX_CHECKPOINTS))
  local i
  for ((i = 0; i < to_prune; i++)); do
    echo "pruning old checkpoint: ${dirs[$i]}"
    rm -rf "${dirs[$i]}"
  done
}

checkpoint_is_restore_verified() {
  local dir="$1"
  local info="${dir}/CHECKPOINT_INFO"
  local restore_verified verified_height verified_stateroot_root verified_against_reference
  local info_height storage_provider state_root_layout

  [[ -d "${dir}/mainnet" ]] || return 1
  [[ -f "$info" ]] || return 1
  [[ ! -e "${dir}/CHECKPOINT_IN_PROGRESS" ]] || return 1

  info_height=$(sed -n 's/^height=//p' "$info" | head -1)
  storage_provider=$(sed -n 's/^storage_provider=//p' "$info" | head -1)
  state_root_layout=$(sed -n 's/^state_root_layout=//p' "$info" | head -1)
  if [[ "${storage_provider,,}" == "mdbx" ]]; then
    [[ "$state_root_layout" == "coordinated_mdbx" ]] || return 1
  else
    [[ -d "${dir}/StateRoot" ]] || return 1
  fi
  restore_verified=$(sed -n 's/^restore_verified=//p' "$info" | head -1)
  verified_height=$(sed -n 's/^verified_height=//p' "$info" | head -1)
  verified_stateroot_root=$(sed -n 's/^verified_stateroot_root=//p' "$info" | head -1)
  verified_against_reference=$(sed -n 's/^verified_against_reference=//p' "$info" | head -1)

  [[ -n "$info_height" ]] || return 1
  [[ "$verified_height" == "$info_height" ]] || return 1
  [[ -n "$verified_stateroot_root" ]] || return 1
  [[ "${restore_verified,,}" == "true" ]] || return 1
  [[ "${verified_against_reference,,}" == "true" ]] || return 1
}

sorted_checkpoints_for_pruning() {
  local dir base height priority
  find "$CHECKPOINT_ROOT" -maxdepth 1 -mindepth 1 -type d -name 'h[0-9]*' 2>/dev/null |
    while IFS= read -r dir; do
      base=$(basename "$dir")
      [[ "$base" =~ ^h([0-9]+)$ ]] || continue
      height="${BASH_REMATCH[1]}"
      if checkpoint_is_restore_verified "$dir"; then
        priority=1
      else
        priority=0
      fi
      printf '%s\t%s\t%s\n' "$priority" "$height" "$dir"
    done |
    sort -n -k1,1 -k2,2 |
    cut -f3-
}

sorted_height_checkpoints() {
  local dir base height
  find "$CHECKPOINT_ROOT" -maxdepth 1 -mindepth 1 -type d -name 'h[0-9]*' 2>/dev/null |
    while IFS= read -r dir; do
      base=$(basename "$dir")
      [[ "$base" =~ ^h([0-9]+)$ ]] || continue
      height="${BASH_REMATCH[1]}"
      printf '%s\t%s\n' "$height" "$dir"
    done |
    sort -n -k1,1 |
    cut -f2-
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
if last_ckpt=$(sorted_height_checkpoints | tail -1); then
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
