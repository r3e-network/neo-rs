#!/bin/bash
# Script to find all non-production-ready code markers in neo-rs
# Usage: ./scripts/find_non_production_code.sh

set -e

echo "=== Neo-rs Non-Production Code Finder ==="
echo ""

# Define patterns to search for
PATTERNS=(
    "TODO"
    "FIXME"
    "HACK"
    "XXX"
    "for now"
    "in real"
    "in production"
    "simplified"
    "placeholder"
    "stub"
    "temporary"
    "temp fix"
    "workaround"
)

# Exclude patterns (test files, comments about mocks in tests)
EXCLUDE_PATHS="target/|\.git/|tests/|_test\.rs|/test"

echo "Searching for non-production markers..."
echo ""

# Count by category
declare -A CATEGORY_COUNTS

for pattern in "${PATTERNS[@]}"; do
    count=$(rg -i "$pattern" --type rust -c 2>/dev/null | grep -vE "$EXCLUDE_PATHS" | awk -F: '{sum += $2} END {print sum+0}')
    CATEGORY_COUNTS["$pattern"]=$count
done

echo "=== Summary by Pattern ==="
for pattern in "${PATTERNS[@]}"; do
    printf "%-20s: %d occurrences\n" "$pattern" "${CATEGORY_COUNTS[$pattern]}"
done

echo ""
echo "=== Detailed Findings (excluding tests) ==="
echo ""

# Production code only (exclude test files)
rg -i "(TODO|FIXME|HACK|XXX)" --type rust -n --no-heading 2>/dev/null | grep -vE "$EXCLUDE_PATHS" || true

echo ""
echo "=== Simplified/Placeholder Implementations ==="
rg -i "(for now|in production|simplified|placeholder|stub|temporary)" --type rust -n --no-heading 2>/dev/null | grep -vE "$EXCLUDE_PATHS" || true

echo ""
echo "=== Files with Most Issues ==="
rg -i "(TODO|FIXME|for now|simplified|placeholder)" --type rust -c 2>/dev/null | grep -vE "$EXCLUDE_PATHS" | sort -t: -k2 -nr | head -10 || true

echo ""
echo "Done."
