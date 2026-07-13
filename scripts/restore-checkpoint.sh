#!/usr/bin/env bash
set -euo pipefail

# Restore live mainnet Ledger + StateService data from an MDBX checkpoint.
# Chain-only checkpoints remain supported when CHECKPOINT_INFO records
# state_root_included=false.
#
# Usage:
#   scripts/restore-checkpoint.sh <height|latest|--at-or-below N> [options]
#
# Options:
#   --root <path>           checkpoint root (default <data-dir>/checkpoints)
#   --data-dir <path>       neo-rs data root (default ./data)
#   --chain-db <path>       explicit chain store target (default <data-dir>/mainnet)
#   --keep-current          keep current data/mainnet as
#                           data/mainnet.prerestore-<TS> instead of deleting
#   --dry-run               print what would happen, do nothing
#   -y, --yes               do not prompt for confirmation
#   --allow-unverified      restore checkpoints without restore verification
#
# Refuses to run when the target MDBX lock file is held by another process.

DATA_DIR="${NEO_DATA_DIR:-./data}"
CHECKPOINT_ROOT="${NEO_CHECKPOINT_ROOT:-}"
CHAIN_DB_OVERRIDE="${NEO_CHAIN_DB:-}"
KEEP_CURRENT=0
DRY_RUN=0
YES=0
ALLOW_UNVERIFIED=0

TARGET="${1:-}"
shift || true
if [[ -z "$TARGET" ]]; then
  sed -n '3,24p' "$0"; exit 1
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
    --chain-db)     CHAIN_DB_OVERRIDE="$2"; shift 2;;
    --keep-current) KEEP_CURRENT=1; shift;;
    --dry-run)      DRY_RUN=1; shift;;
    -y|--yes)       YES=1; shift;;
    --allow-unverified) ALLOW_UNVERIFIED=1; shift;;
    *) echo "unknown option: $1" >&2; exit 1;;
  esac
done

CHECKPOINT_ROOT="${CHECKPOINT_ROOT:-${DATA_DIR}/checkpoints}"
CHAIN_DB="${CHAIN_DB_OVERRIDE:-${DATA_DIR}/mainnet}"

if [[ ! -d "$CHECKPOINT_ROOT" ]]; then
  echo "no checkpoint root at $CHECKPOINT_ROOT" >&2; exit 1
fi

# ----- pick checkpoint dir -----
dir_height() {
  local dir="$1"
  local base info_height
  base=$(basename "$dir")
  case "$base" in
    h[0-9]*)
      printf '%s\n' "${base#h}"
      return
      ;;
  esac
  return 1
}

checkpoint_dirs_by_height() {
  local d height
  while IFS= read -r d; do
    height=$(dir_height "$d" || true)
    if [[ -n "$height" ]]; then
      printf '%s\t%s\n' "$height" "$d"
    fi
  done < <(find "$CHECKPOINT_ROOT" -mindepth 1 -maxdepth 1 -type d 2>/dev/null)
}

metadata_value_for_dir() {
  local dir="$1"
  local key="$2"
  local file="$dir/CHECKPOINT_INFO"
  [[ -f "$file" ]] || return 1
  sed -n "s/^${key}=//p" "$file" | head -1
}

checkpoint_has_stateroot_dir() {
  local dir="$1"
  local provider layout included
  provider=$(metadata_value_for_dir "$dir" storage_provider || true)
  layout=$(metadata_value_for_dir "$dir" state_root_layout || true)
  included=$(metadata_value_for_dir "$dir" state_root_included || true)
  [[ "${provider,,}" == "mdbx" ]] || return 1
  [[ "$layout" == "coordinated_mdbx" ]] || return 1
  [[ "${included,,}" == "true" ]]
}

checkpoint_is_verified_full_state() {
  local dir="$1"
  local height verified_height verified_stateroot_root restore_verified verified_against_reference
  [[ -d "$dir" ]] || return 1
  [[ -f "$dir/CHECKPOINT_INFO" ]] || return 1
  [[ ! -f "$dir/CHECKPOINT_IN_PROGRESS" ]] || return 1
  [[ -d "$dir/mainnet" ]] || return 1
  checkpoint_has_stateroot_dir "$dir" || return 1

  height=$(metadata_value_for_dir "$dir" height || true)
  verified_height=$(metadata_value_for_dir "$dir" verified_height || true)
  verified_stateroot_root=$(metadata_value_for_dir "$dir" verified_stateroot_root || true)
  restore_verified=$(metadata_value_for_dir "$dir" restore_verified || true)
  verified_against_reference=$(metadata_value_for_dir "$dir" verified_against_reference || true)

  [[ -n "$height" && -n "$verified_height" && "$height" == "$verified_height" ]] || return 1
  [[ -n "$verified_stateroot_root" ]] || return 1
  [[ "${restore_verified,,}" == "true" ]] || return 1
  [[ "${verified_against_reference,,}" == "true" ]] || return 1
}

candidate_allowed_for_dynamic_restore() {
  local dir="$1"
  if [[ "$ALLOW_UNVERIFIED" -eq 1 ]]; then
    return 0
  fi
  checkpoint_is_verified_full_state "$dir"
}

print_available_checkpoints() {
  local height d found=0
  while IFS=$'\t' read -r height d; do
    found=1
    printf '  h%s\t%s\n' "$height" "$d" >&2
  done < <(checkpoint_dirs_by_height | sort -n -k1,1)
  if [[ "$found" -eq 0 ]]; then
    echo "  (none)" >&2
  fi
}

pick_dir() {
  local req="$1"
  if [[ "$req" == "latest" ]]; then
    local height d picked=""
    while IFS=$'\t' read -r height d; do
      if candidate_allowed_for_dynamic_restore "$d"; then picked="$d"; fi
    done < <(checkpoint_dirs_by_height | sort -n -k1,1)
    printf '%s\n' "$picked"
    return
  fi
  if [[ -n "$AT_OR_BELOW" ]]; then
    local height d picked=""
    while IFS=$'\t' read -r height d; do
      if (( height <= AT_OR_BELOW )) && candidate_allowed_for_dynamic_restore "$d"; then picked="$d"; fi
    done < <(checkpoint_dirs_by_height | sort -n -k1,1)
    printf '%s\n' "$picked"
    return
  fi
  # exact height
  local d
  if [[ "$req" == *[!0-9]* ]]; then
    return
  fi
  d="${CHECKPOINT_ROOT}/h${req}"
  if [[ -d "$d" ]]; then printf '%s\n' "$d"; fi
}

if [[ "$TARGET" == "--at-or-below" ]]; then
  SOURCE=$(pick_dir "")
elif [[ "$TARGET" == "latest" ]]; then
  SOURCE=$(pick_dir latest)
else
  SOURCE=$(pick_dir "$TARGET")
fi

if [[ -z "$SOURCE" || ! -d "$SOURCE" ]]; then
  echo "no matching checkpoint found in $CHECKPOINT_ROOT" >&2
  echo "available:" >&2
  print_available_checkpoints
  exit 1
fi

metadata_value() {
  local key="$1"
  local file="$SOURCE/CHECKPOINT_INFO"
  [[ -f "$file" ]] || return 1
  sed -n "s/^${key}=//p" "$file" | head -1
}

SOURCE_BASENAME=$(basename "$SOURCE")
SOURCE_HEIGHT=$(printf '%s' "$SOURCE_BASENAME" | sed -n 's/^h\([0-9][0-9]*\)$/\1/p')
if [[ -z "$SOURCE_HEIGHT" ]]; then
  SOURCE_HEIGHT=$(metadata_value height || true)
fi

SOURCE_STORAGE_PROVIDER=$(metadata_value storage_provider || true)
SOURCE_STATE_ROOT_LAYOUT=$(metadata_value state_root_layout || true)
if [[ -z "$SOURCE_HEIGHT" ]]; then
  echo "checkpoint $SOURCE is missing a numeric height in its name or CHECKPOINT_INFO" >&2; exit 1
fi

SOURCE_HAS_STATEROOT=1
SOURCE_CHAIN_DIR=""
if [[ -d "$SOURCE/mainnet" ]]; then
  SOURCE_CHAIN_DIR="$SOURCE/mainnet"
else
  echo "checkpoint $SOURCE is incomplete (missing mainnet/)" >&2; exit 1
fi
if [[ "${SOURCE_STORAGE_PROVIDER,,}" != "mdbx" || "$SOURCE_STATE_ROOT_LAYOUT" != "coordinated_mdbx" ]]; then
  echo "checkpoint $SOURCE is not a coordinated MDBX checkpoint" >&2
  exit 1
fi
if [[ -f "$SOURCE/CHECKPOINT_INFO" ]] && grep -qx 'state_root_included=false' "$SOURCE/CHECKPOINT_INFO"; then
  SOURCE_HAS_STATEROOT=0
fi
if [[ -f "$SOURCE/CHECKPOINT_IN_PROGRESS" ]]; then
  echo "checkpoint $SOURCE was not finalized (CHECKPOINT_IN_PROGRESS marker present)" >&2; exit 1
fi

verification_reason() {
  local file="$SOURCE/CHECKPOINT_INFO"
  local height verified_height verified_stateroot_root restore_verified verified_against_reference
  [[ -f "$file" ]] || { echo "missing restore verification metadata: CHECKPOINT_INFO" >&2; return 1; }
  height=$(sed -n 's/^height=//p' "$file" | head -1)
  verified_height=$(sed -n 's/^verified_height=//p' "$file" | head -1)
  verified_stateroot_root=$(sed -n 's/^verified_stateroot_root=//p' "$file" | head -1)
  restore_verified=$(sed -n 's/^restore_verified=//p' "$file" | head -1)
  verified_against_reference=$(sed -n 's/^verified_against_reference=//p' "$file" | head -1)

  if [[ -z "$verified_height" || -z "$verified_stateroot_root" || -z "$restore_verified" || -z "$verified_against_reference" ]]; then
    local missing=()
    [[ -z "$restore_verified" ]] && missing+=("restore_verified")
    [[ -z "$verified_height" ]] && missing+=("verified_height")
    [[ -z "$verified_stateroot_root" ]] && missing+=("verified_stateroot_root")
    [[ -z "$verified_against_reference" ]] && missing+=("verified_against_reference")
    echo "missing restore verification metadata: ${missing[*]}" >&2
    return 1
  fi

  if [[ "$height" != "$verified_height" ]]; then
    echo "restore verification height does not match checkpoint height: height=${height}, verified_height=${verified_height}" >&2
    return 1
  fi

  if [[ "${restore_verified,,}" != "true" ]]; then
    echo "restore verification metadata is not marked restore_verified=true" >&2
    return 1
  fi

  if [[ "${verified_against_reference,,}" != "true" ]]; then
    echo "restore verification metadata is not marked verified_against_reference=true" >&2
    return 1
  fi
}

if [[ "$SOURCE_HAS_STATEROOT" -eq 1 && "$ALLOW_UNVERIFIED" -eq 0 ]]; then
  if ! verification_reason; then
    echo "refusing to restore unverified checkpoint $SOURCE" >&2
    echo "use --allow-unverified only for internal restore-probe verification flows" >&2
    exit 1
  fi
fi

lock_is_held() {
  local lock="$1"
  if command -v fuser >/dev/null 2>&1; then
    fuser "$lock" >/dev/null 2>&1
    return
  fi
  if command -v lsof >/dev/null 2>&1; then
    lsof "$lock" >/dev/null 2>&1
    return
  fi
  return 2
}

# Refuse only when the target store's lock file is actively held. Other
# neo-node processes may legitimately be running against unrelated data dirs.
LOCK_PATHS=("$CHAIN_DB/mdbx.lck")
for lock in "${LOCK_PATHS[@]}"; do
  if [[ -f "$lock" ]]; then
    if lock_is_held "$lock"; then
      echo "ERROR: $lock is held by another process; refusing to restore." >&2
      exit 1
    else
      lock_status=$?
      if [[ "$lock_status" -eq 2 ]]; then
        echo "ERROR: cannot inspect $lock; install fuser or lsof before restoring." >&2
        exit 1
      fi
    fi
  fi
done

echo "restoring from checkpoint h${SOURCE_HEIGHT} (${SOURCE})"
echo "  -> $CHAIN_DB"
if [[ "$SOURCE_HAS_STATEROOT" -eq 1 ]]; then
  echo "  -> StateService table included in $CHAIN_DB"
else
  echo "  -> StateService table not included (chain-only checkpoint)"
fi
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

copy_store_dir() {
  local src="$1"
  local dst="$2"
  cp -a --reflink=auto "$src" "$dst" 2>/dev/null || cp -a "$src" "$dst"
}

mkdir -p "$(dirname "$CHAIN_DB")"
copy_store_dir "$SOURCE_CHAIN_DIR" "$CHAIN_DB"

if [[ "$SOURCE_HAS_STATEROOT" -eq 1 ]]; then
  echo "restore complete. chain & state DB are now at height ~${SOURCE_HEIGHT}."
else
  echo "restore complete. chain DB is now at height ~${SOURCE_HEIGHT}; StateService table not included."
fi
echo "Run neo-db-probe to inspect the restored store:"
echo "  cargo run -p neo-node --bin neo-db-probe --release -- --db $CHAIN_DB --contract-id -4 --key-hex 0c"
