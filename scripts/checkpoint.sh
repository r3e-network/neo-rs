#!/usr/bin/env bash
#
# checkpoint.sh — snapshot / restore the node's synced RocksDB state so that
# investigating a sync divergence never requires re-syncing from genesis.
#
# The node commits every block to RocksDB before advancing (durability is
# per-block), so a clone of a STOPPED node's data directory is a consistent
# checkpoint at the exact synced height. On APFS the clone is instant and
# copy-on-write (it shares blocks with the live DB until they diverge), so
# keeping several checkpoints is cheap.
#
# Usage:
#   scripts/checkpoint.sh save    <label> [data_dir] [mpt_dir]
#   scripts/checkpoint.sh restore <label> [data_dir] [mpt_dir]
#   scripts/checkpoint.sh list
#   scripts/checkpoint.sh height  [rpc_url]      # print the live node's height
#
# Defaults: data_dir=data/mainnet-validate  mpt_dir=Data_MPT_validate_334F454E
# Checkpoints live under ./checkpoints/<label>/.
#
# Typical workflow:
#   1. While a long validation sync is healthy, snapshot milestones:
#        scripts/checkpoint.sh height            # e.g. 300000
#        pkill -f neo_mainnet_validate.toml      # stop the node
#        scripts/checkpoint.sh save 300000
#        ./target/release/neo-node --config neo_mainnet_validate.toml &  # resume
#   2. When the sync stalls at block X with a root cause at an earlier block Y,
#      restore the nearest checkpoint below Y and re-sync (with instrumentation)
#      from there instead of from 0:
#        pkill -f neo_mainnet_validate.toml
#        scripts/checkpoint.sh restore 300000
#        ./target/release/neo-node --config neo_mainnet_validate.toml &
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
CKPT_DIR="$ROOT/checkpoints"
DEFAULT_DATA="data/mainnet-validate"
DEFAULT_MPT="Data_MPT_validate_334F454E"
DEFAULT_RPC="http://127.0.0.1:20332"

# Clone a directory tree fast (APFS clonefile) with a portable fallback.
clone_tree() {
  local src="$1" dst="$2"
  [ -e "$src" ] || return 0
  rm -rf "$dst"
  if cp -c -R "$src" "$dst" 2>/dev/null; then :; else cp -R "$src" "$dst"; fi
}

node_using() { # warn if a neo-node process still holds the data dir
  if pgrep -f "neo-node" >/dev/null 2>&1; then
    echo "WARNING: a neo-node process is running. Stop it first (pkill -f neo-node)"
    echo "         — cloning a live RocksDB directory yields an inconsistent checkpoint."
    return 1
  fi
  return 0
}

rpc_height() {
  curl -s --max-time 6 -X POST "${1:-$DEFAULT_RPC}" \
    -H 'Content-Type: application/json' \
    -d '{"jsonrpc":"2.0","method":"getblockcount","params":[],"id":1}' \
    | python3 -c 'import sys,json;print(json.load(sys.stdin)["result"])' 2>/dev/null || echo "?"
}

cmd="${1:-}"; shift || true
case "$cmd" in
  save)
    label="${1:?usage: save <label> [data_dir] [mpt_dir]}"
    data="${2:-$DEFAULT_DATA}"; mpt="${3:-$DEFAULT_MPT}"
    node_using || exit 1
    dest="$CKPT_DIR/$label"
    mkdir -p "$dest"
    echo "Saving checkpoint '$label' from $data ..."
    clone_tree "$data" "$dest/data"
    [ -e "$mpt" ] && clone_tree "$mpt" "$dest/mpt" || true
    # record provenance
    {
      echo "label=$label"
      echo "saved_at=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
      echo "data_dir=$data"
      echo "mpt_dir=$mpt"
    } > "$dest/CHECKPOINT_INFO"
    echo "Done. $(du -sh "$dest" 2>/dev/null | awk '{print $1}') at $dest"
    echo "Restore with: scripts/checkpoint.sh restore $label $data $mpt"
    ;;
  restore)
    label="${1:?usage: restore <label> [data_dir] [mpt_dir]}"
    data="${2:-$DEFAULT_DATA}"; mpt="${3:-$DEFAULT_MPT}"
    src="$CKPT_DIR/$label"
    [ -d "$src/data" ] || { echo "no such checkpoint: $src"; exit 1; }
    node_using || exit 1
    echo "Restoring checkpoint '$label' -> $data ..."
    clone_tree "$src/data" "$data"
    [ -e "$src/mpt" ] && clone_tree "$src/mpt" "$mpt" || true
    echo "Done. Start the node and it resumes from the checkpoint height."
    ;;
  list)
    mkdir -p "$CKPT_DIR"
    printf "%-20s %-8s %s\n" "LABEL" "SIZE" "SAVED_AT"
    for d in "$CKPT_DIR"/*/; do
      [ -d "$d" ] || continue
      l="$(basename "$d")"
      sz="$(du -sh "$d" 2>/dev/null | awk '{print $1}')"
      ts="$(grep -h '^saved_at=' "$d/CHECKPOINT_INFO" 2>/dev/null | cut -d= -f2)"
      printf "%-20s %-8s %s\n" "$l" "${sz:-?}" "${ts:-?}"
    done
    ;;
  height)
    rpc_height "${1:-$DEFAULT_RPC}"
    ;;
  *)
    sed -n '2,40p' "$0" | sed 's/^# \{0,1\}//'
    exit 1
    ;;
esac
