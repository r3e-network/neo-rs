#!/usr/bin/env bash
set -euo pipefail

# List available chain+StateRoot checkpoints in pretty form.
#
# Usage: scripts/list-checkpoints.sh [--root <path>] [--data-dir <path>]

DATA_DIR="${NEO_DATA_DIR:-./data}"
CHECKPOINT_ROOT="${NEO_CHECKPOINT_ROOT:-}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --root)     CHECKPOINT_ROOT="$2"; shift 2;;
    --data-dir) DATA_DIR="$2"; shift 2;;
    -h|--help)  sed -n '3,8p' "$0"; exit 0;;
    *) echo "unknown option: $1" >&2; exit 1;;
  esac
done

CHECKPOINT_ROOT="${CHECKPOINT_ROOT:-${DATA_DIR}/checkpoints}"

if [[ ! -d "$CHECKPOINT_ROOT" ]]; then
  echo "no checkpoint root at $CHECKPOINT_ROOT" >&2; exit 1
fi

printf '%-10s  %-21s  %-10s  %-10s  %s\n' "HEIGHT" "COMPLETED" "DU-CHAIN" "DU-STATE" "PATH"
shopt -s nullglob
mapfile -t dirs < <(printf '%s\n' "${CHECKPOINT_ROOT}"/h[0-9]* | sort -t h -k2,2n)
if [[ ${#dirs[@]} -eq 0 ]]; then
  echo "(no checkpoints)"
  exit 0
fi
for d in "${dirs[@]}"; do
  h=$(basename "$d" | sed 's/^h//')
  if [[ -f "$d/CHECKPOINT_IN_PROGRESS" ]]; then
    completed="IN-PROGRESS"
  elif [[ -f "$d/CHECKPOINT_INFO" ]]; then
    completed=$(sed -n 's/^completed_at=//p' "$d/CHECKPOINT_INFO" | head -1)
    completed="${completed:-?}"
  else
    completed="?"
  fi
  du_chain=$(du -sh "$d/mainnet" 2>/dev/null | awk '{print $1}')
  du_state=$(du -sh "$d/StateRoot" 2>/dev/null | awk '{print $1}')
  printf '%-10s  %-21s  %-10s  %-10s  %s\n' "$h" "${completed}" "${du_chain:--}" "${du_state:--}" "$d"
done
