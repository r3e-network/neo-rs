#!/bin/bash
# Neo-RS Security Check Script
# Run this script as part of CI/CD pipeline to catch security issues early

set -e

echo "=========================================="
echo "Neo-RS Security Check"
echo "=========================================="

ERRORS=0
WARNINGS=0

error() { echo "[ERROR] $1"; ((ERRORS++)) || true; }
warn() { echo "[WARN] $1"; ((WARNINGS++)) || true; }
success() { echo "[OK] $1"; }

echo ""
echo "1. Checking for insecure RNG in key generation..."
echo "--------------------------------------------------"

if grep -q "thread_rng" neo-core/src/wallets/key_pair.rs 2>/dev/null; then
    error "Found thread_rng() in key_pair.rs (should use OsRng)"
else
    success "key_pair.rs uses secure RNG"
fi

echo ""
echo "2. Checking BigInt size limits in VM..."
echo "----------------------------------------"

if grep -q "check_bigint_size" neo-vm/src/jump_table/numeric.rs 2>/dev/null; then
    success "BigInt size checks are in place"
else
    error "Missing BigInt size checks in numeric.rs"
fi

echo ""
echo "3. Checking for unsafe blocks count..."
echo "---------------------------------------"

UNSAFE_COUNT=$(grep -r "unsafe" --include="*.rs" neo-vm/src/ 2>/dev/null | wc -l)
echo "Found $UNSAFE_COUNT unsafe usages in neo-vm"

if [ "$UNSAFE_COUNT" -gt 50 ]; then
    warn "High number of unsafe blocks"
else
    success "Acceptable unsafe count"
fi

echo ""
echo "4. Verifying compilation..."
echo "---------------------------"

if cargo check -p neo-vm -p neo-core --quiet 2>/dev/null; then
    success "Compilation passed"
else
    error "Compilation failed"
fi

echo ""
echo "=========================================="
echo "Summary: Errors=$ERRORS, Warnings=$WARNINGS"
echo "=========================================="

if [ $ERRORS -gt 0 ]; then
    echo "FAILED"
    exit 1
else
    echo "PASSED"
    exit 0
fi
