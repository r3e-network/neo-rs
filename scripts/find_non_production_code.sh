#!/usr/bin/env bash
# Script to find non-production-ready markers in neo-rs.
#
# Usage:
#   ./scripts/find_non_production_code.sh              # scan Rust production code (default)
#   ./scripts/find_non_production_code.sh --all        # include tests + docs + configs
#   ./scripts/find_non_production_code.sh --fail       # non-zero exit if findings
#
# Notes:
# - "Hard" markers (TODO/FIXME/HACK/XXX) are always reported.
# - Rust hard markers also include `todo!()` / `unimplemented!()`.
# - "Soft" markers ("for now", "placeholder", "simplified", "in production", etc.) are only
#   reported when they appear in comments/docstrings to avoid false positives on identifiers.

set -e

echo "=== Neo-rs Non-Production Code Finder ==="
echo ""

INCLUDE_ALL=0
FAIL=0
for arg in "$@"; do
  case "$arg" in
    --all) INCLUDE_ALL=1 ;;
    --fail) FAIL=1 ;;
    *) echo "Unknown argument: $arg" >&2; exit 2 ;;
  esac
done

EXCLUDE_GLOBS=(
  --glob '!target/**'
  --glob '!.git/**'
  --glob '!neo_csharp/**'
  --glob '!.cargo-ai/**'
  --glob '!.claude/**'
)

if [[ "$INCLUDE_ALL" -eq 0 ]]; then
  EXCLUDE_GLOBS+=(
    --glob '!tests/**'
    --glob '!docs/**'
    --glob '!**/tests/**'
    --glob '!**/docs/**'
    --glob '!**/*_test.rs'
  )
fi

HARD_REGEX='\b(TODO|FIXME|HACK|XXX)\b|\b(todo!|unimplemented!)\s*\('
SOFT_REGEX='(for now|in production|in real implementation|simplified|placeholder|stub|not production|non-production)'
SOFT_IN_COMMENTS_REGEX="(^|\\s)(//|///|//!|/\\*|\\*)\\s*.*${SOFT_REGEX}"

echo "Searching for non-production markers..."
echo ""

hard_count=0
if [[ "$INCLUDE_ALL" -eq 0 ]]; then
  hard_count=$(rg -n -S --hidden "${EXCLUDE_GLOBS[@]}" --type rust "$HARD_REGEX" . | wc -l | tr -d ' ')
else
  hard_count=$(rg -n -S --hidden "${EXCLUDE_GLOBS[@]}" "$HARD_REGEX" . | wc -l | tr -d ' ')
fi
soft_rust_count=$(rg -n -S --hidden "${EXCLUDE_GLOBS[@]}" --type rust -i "$SOFT_IN_COMMENTS_REGEX" . | wc -l | tr -d ' ')
soft_text_count=0
if [[ "$INCLUDE_ALL" -eq 1 ]]; then
  CFG_GLOBS=( -g '*.toml' -g '*.json' -g '*.yaml' -g '*.yml' -g '*.ini' -g '*.conf' -g '*.txt' -g '*.md' )
  soft_text_count=$(rg -n -S --hidden "${EXCLUDE_GLOBS[@]}" "${CFG_GLOBS[@]}" -i "$SOFT_REGEX" . | wc -l | tr -d ' ')
fi

echo "=== Summary ==="
printf "%-22s: %d occurrences\n" "Hard markers" "$hard_count"
printf "%-22s: %d occurrences\n" "Soft markers (Rust comments)" "$soft_rust_count"
if [[ "$INCLUDE_ALL" -eq 1 ]]; then
  printf "%-22s: %d occurrences\n" "Soft markers (docs/config)" "$soft_text_count"
fi

echo ""
echo "=== Detailed Findings ==="
echo ""

if [[ "$INCLUDE_ALL" -eq 0 ]]; then
  rg -n -S --hidden "${EXCLUDE_GLOBS[@]}" --type rust "$HARD_REGEX" . || true
else
  rg -n -S --hidden "${EXCLUDE_GLOBS[@]}" "$HARD_REGEX" . || true
fi

echo ""
echo "=== Soft Markers (Rust comments/docstrings only) ==="
rg -n -S --type rust --hidden "${EXCLUDE_GLOBS[@]}" -i "$SOFT_IN_COMMENTS_REGEX" . || true

if [[ "$INCLUDE_ALL" -eq 1 ]]; then
  echo ""
  echo "=== Soft Markers (docs/config) ==="
  rg -n -S --hidden "${EXCLUDE_GLOBS[@]}" "${CFG_GLOBS[@]}" -i "$SOFT_REGEX" . || true
fi

echo ""
echo "Done."

if [[ "$FAIL" -eq 1 && ( "$hard_count" -ne 0 || "$soft_rust_count" -ne 0 || "$soft_text_count" -ne 0 ) ]]; then
  exit 1
fi
