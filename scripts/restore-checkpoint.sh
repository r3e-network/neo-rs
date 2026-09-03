#!/usr/bin/env bash
set -euo pipefail

# Restore live mainnet chain DB + StateRoot DB from a previously-taken
# checkpoint. Hardlinks the SST files back into place, which is cheap on the
# same filesystem (the source checkpoint files are not touched — they remain
# the canonical copies if you ever need to re-restore).
#
# Usage:
#   scripts/restore-checkpoint.sh <height|latest|--at-or-below N> [options]
#
# Options:
#   --root <path>           checkpoint root (default <data-dir>/checkpoints)
#   --data-dir <path>       neo-rs data root (default ./data)
#   --keep-current          keep current data/mainnet & StateRoot as
#                           data/mainnet.prerestore-<TS> instead of deleting
#   --dry-run               print what would happen, do nothing
#   -y, --yes               do not prompt for confirmation
#
# Refuses to run while a neo-node writer is running against the data dir.

DATA_DIR="${NEO_DATA_DIR:-./data}"
CHECKPOINT_ROOT="${NEO_CHECKPOINT_ROOT:-}"
KEEP_CURRENT=0
DRY_RUN=0
YES=0

TARGET="${1:-}"
shift || true
if [[ -z "$TARGET" ]]; then
  sed -n '3,18p' "$0"; exit 1
fi

# `--at-or-below N` means "pick the highest checkpoint with height <= N"
AT_OR_BELOW=""
if [[ "$TARGET" == "--at-or-below" ]]; then
  AT_OR_BELOW="${1:-}"; shift || true
  if [[ -z "$AT_OR_BELOW" ]]; then echo "--at-or-below needs a height"; exit 1; fi
fi

while [[ $# -gt 0 ]]; do
  case "$1" in
    --root)         CHECKPOINT_ROOT="$2"; shift 2;;
    --data-dir)     DATA_DIR="$2"; shift 2;;
    --keep-current) KEEP_CURRENT=1; shift;;
    --dry-run)      DRY_RUN=1; shift;;
    -y|--yes)       YES=1; shift;;
    *) echo "unknown option: $1" >&2; exit 1;;
  esac
done

CHECKPOINT_ROOT="${CHECKPOINT_ROOT:-${DATA_DIR}/checkpoints}"
CHAIN_DB="${DATA_DIR}/mainnet"
STATEROOT_DB="${DATA_DIR}/Plugins/mainnet/StateRoot"

if [[ ! -d "$CHECKPOINT_ROOT" ]]; then
  echo "no checkpoint root at $CHECKPOINT_ROOT" >&2; exit 1
fi

# ----- pick checkpoint dir -----
pick_dir() {
  local req="$1"
  if [[ "$req" == "latest" ]]; then
    ls -1d "${CHECKPOINT_ROOT}"/h[0-9]* 2>/dev/null | sort -t h -k2,2n | tail -1
    return
  fi
  if [[ -n "$AT_OR_BELOW" ]]; then
    local picked=""
    while read -r d; do
      h=$(basename "$d" | sed 's/^h//')
      if (( h <= AT_OR_BELOW )); then picked="$d"; fi
    done < <(ls -1d "${CHECKPOINT_ROOT}"/h[0-9]* 2>/dev/null | sort -t h -k2,2n)
    printf '%s\n' "$picked"
    return
  fi
  # exact height
  local d="${CHECKPOINT_ROOT}/h${req}"
  if [[ -d "$d" ]]; then printf '%s\n' "$d"; fi
}

if [[ "$TARGET" == "--at-or-below" ]]; then
  SOURCE=$(pick_dir "")
elif [[ "$TARGET" == "latest" ]]; then
  SOURCE=$(pick_dir latest)
else
  case "$TARGET" in
    ''|*[!0-9]*) echo "<height> must be a non-negative integer, 'latest', or '--at-or-below N'"; exit 1;;
  esac
  SOURCE=$(pick_dir "$TARGET")
fi

if [[ -z "$SOURCE" || ! -d "$SOURCE" ]]; then
  echo "no matching checkpoint found in $CHECKPOINT_ROOT" >&2
  echo "available:" >&2
  ls -1d "${CHECKPOINT_ROOT}"/h[0-9]* 2>/dev/null | sed 's|^|  |' >&2 || echo "  (none)" >&2
  exit 1
fi

SOURCE_HEIGHT=$(basename "$SOURCE" | sed 's/^h//')
if [[ ! -d "$SOURCE/mainnet" || ! -d "$SOURCE/StateRoot" ]]; then
  echo "checkpoint $SOURCE is incomplete (missing mainnet/ or StateRoot/)" >&2; exit 1
fi
if [[ -f "$SOURCE/CHECKPOINT_IN_PROGRESS" ]]; then
  echo "checkpoint $SOURCE was not finalized (CHECKPOINT_IN_PROGRESS marker present)" >&2; exit 1
fi

# ----- safety: refuse if writer is live -----
PIDS=$(pgrep -f 'neo-node|neo-cli' || true)
if [[ -n "$PIDS" ]]; then
  echo "ERROR: a neo-node/neo-cli process appears to be running:" >&2
  ps -fp $PIDS >&2 || true
  echo "Stop it first, then re-run." >&2
  exit 1
fi

# Optional lock-file from the syncer
for lock in "$CHAIN_DB/LOCK" "$STATEROOT_DB/LOCK"; do
  if [[ -f "$lock" ]] && fuser "$lock" >/dev/null 2>&1; then
    echo "ERROR: $lock is held by another process; refusing to restore." >&2
    exit 1
  fi
done

echo "restoring from checkpoint h${SOURCE_HEIGHT} (${SOURCE})"
echo "  -> $CHAIN_DB"
echo "  -> $STATEROOT_DB"
if [[ "$KEEP_CURRENT" -eq 1 ]]; then
  TS=$(date +%Y%m%d-%H%M%S)
  echo "  current dirs will be renamed to .prerestore-${TS}/ (not deleted)"
else
  echo "  current dirs will be DELETED"
fi
if [[ "$DRY_RUN" -eq 1 ]]; then echo "(dry-run, exiting)"; exit 0; fi

if [[ "$YES" -ne 1 ]]; then
  read -r -p "proceed? [y/N] " ans
  case "$ans" in y|Y|yes|YES) ;; *) echo "aborted"; exit 1;; esac
fi

stash_or_rm() {
  local dir="$1"
  if [[ -d "$dir" ]]; then
    if [[ "$KEEP_CURRENT" -eq 1 ]]; then
      local ts="${TS:-$(date +%Y%m%d-%H%M%S)}"
      mv "$dir" "${dir}.prerestore-${ts}"
    else
      rm -rf "$dir"
    fi
  fi
}

stash_or_rm "$CHAIN_DB"
stash_or_rm "$STATEROOT_DB"

mkdir -p "$(dirname "$CHAIN_DB")" "$(dirname "$STATEROOT_DB")"
cp -al "$SOURCE/mainnet"   "$CHAIN_DB"     2>/dev/null || cp -a "$SOURCE/mainnet"   "$CHAIN_DB"
cp -al "$SOURCE/StateRoot" "$STATEROOT_DB" 2>/dev/null || cp -a "$SOURCE/StateRoot" "$STATEROOT_DB"

echo "restore complete. chain & state DB are now at height ~${SOURCE_HEIGHT}."
echo "Run print_height to confirm:"
echo "  cargo run -p neo-core --example print_height --release -- $CHAIN_DB"
