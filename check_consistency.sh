#!/bin/bash

# Neo N3 Rust Implementation Consistency Check Script
# This script performs comprehensive checks for consistency, completeness, and correctness

echo "==============================================="
echo "Neo N3 Rust Implementation Consistency Check"
echo "==============================================="
echo ""

# Color codes for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Counters
ERRORS=0
WARNINGS=0
SUCCESS=0

# Function to print colored output
print_status() {
    if [ "$1" = "ERROR" ]; then
        echo -e "${RED}[ERROR]${NC} $2"
        ((ERRORS++))
    elif [ "$1" = "WARNING" ]; then
        echo -e "${YELLOW}[WARNING]${NC} $2"
        ((WARNINGS++))
    elif [ "$1" = "SUCCESS" ]; then
        echo -e "${GREEN}[SUCCESS]${NC} $2"
        ((SUCCESS++))
    fi
}

echo "1. Checking Compilation Status..."
echo "=================================="
if cargo build --all 2>&1 | grep -q "error:"; then
    print_status "ERROR" "Compilation errors found"
else
    print_status "SUCCESS" "All crates compile successfully"
fi

echo ""
echo "2. Checking for Unimplemented Code..."
echo "======================================"
UNIMPLEMENTED=$(grep -r "todo!()\|unimplemented!()\|NotImplemented" crates --include="*.rs" | wc -l)
if [ "$UNIMPLEMENTED" -gt 0 ]; then
    print_status "WARNING" "Found $UNIMPLEMENTED unimplemented functions"
    echo "  Locations:"
    grep -r "todo!()\|unimplemented!()\|NotImplemented" crates --include="*.rs" | head -5 | sed 's/^/    /'
else
    print_status "SUCCESS" "No unimplemented placeholders found"
fi

echo ""
echo "3. Checking Cross-Crate Dependencies..."
echo "========================================"
# Check if core types are used consistently
CORE_TYPES=("Transaction" "Block" "UInt160" "UInt256" "Witness" "Signer")
for TYPE in "${CORE_TYPES[@]}"; do
    COUNT=$(grep -r "use neo_core::.*$TYPE" crates --include="*.rs" | wc -l)
    if [ "$COUNT" -gt 0 ]; then
        print_status "SUCCESS" "$TYPE is imported from neo_core in $COUNT files"
    fi
done

echo ""
echo "4. Checking Error Handling Consistency..."
echo "========================================="
# Check if all crates have proper error types
CRATES=("core" "network" "ledger" "vm" "consensus" "smart_contract" "persistence")
for CRATE in "${CRATES[@]}"; do
    if grep -q "pub enum Error" crates/$CRATE/src/lib.rs 2>/dev/null || grep -q "pub enum Error" crates/$CRATE/src/error.rs 2>/dev/null; then
        print_status "SUCCESS" "$CRATE has proper error type definition"
    else
        print_status "WARNING" "$CRATE might be missing error type definition"
    fi
done

echo ""
echo "5. Checking Protocol Constants..."
echo "================================="
# Check if constants match C# Neo
if grep -q "MAX_TRANSACTION_SIZE.*102400" crates/config/src/lib.rs; then
    print_status "SUCCESS" "MAX_TRANSACTION_SIZE matches C# (102400)"
else
    print_status "ERROR" "MAX_TRANSACTION_SIZE doesn't match C# value"
fi

if grep -q "MAX_BLOCK_SIZE.*2097152" crates/config/src/lib.rs; then
    print_status "SUCCESS" "MAX_BLOCK_SIZE matches C# (2097152)"
else
    print_status "ERROR" "MAX_BLOCK_SIZE doesn't match C# value"
fi

echo ""
echo "6. Checking Native Contract Hashes..."
echo "====================================="
# Check native contract hashes
if grep -q "ef4073a0f2b305a38ec4050e4d3d28bc40ea63f5" crates/node/src/native_contracts.rs; then
    print_status "SUCCESS" "NEO contract hash is correct"
else
    print_status "ERROR" "NEO contract hash is incorrect"
fi

if grep -q "d2a4cff31913016155e38e474a2c06d08be276cf" crates/node/src/native_contracts.rs; then
    print_status "SUCCESS" "GAS contract hash is correct"
else
    print_status "ERROR" "GAS contract hash is incorrect"
fi

echo ""
echo "7. Checking VM OpCode Correctness..."
echo "===================================="
# Check critical opcodes
if grep -q "CAT = 0x8B" crates/vm/src/op_code/op_code.rs; then
    print_status "SUCCESS" "CAT opcode value is correct (0x8B)"
else
    print_status "ERROR" "CAT opcode value is incorrect"
fi

if grep -q "SUBSTR = 0x8C" crates/vm/src/op_code/op_code.rs; then
    print_status "SUCCESS" "SUBSTR opcode value is correct (0x8C)"
else
    print_status "ERROR" "SUBSTR opcode value is incorrect"
fi

echo ""
echo "8. Checking Network Protocol..."
echo "================================"
# Check ExtensiblePayload implementation
if [ -f "crates/network/src/messages/extensible_payload.rs" ]; then
    print_status "SUCCESS" "ExtensiblePayload is implemented"
else
    print_status "ERROR" "ExtensiblePayload is missing"
fi

# Check if Consensus command 0x41 is removed
if grep -q "Consensus = 0x41" crates/network/src/messages/commands.rs; then
    print_status "ERROR" "Invalid Consensus command (0x41) still present"
else
    print_status "SUCCESS" "Consensus command (0x41) correctly removed"
fi

echo ""
echo "9. Checking Test Coverage..."
echo "============================"
TEST_FILES=$(find crates -name "*test*.rs" -o -name "tests.rs" | wc -l)
if [ "$TEST_FILES" -gt 50 ]; then
    print_status "SUCCESS" "Found $TEST_FILES test files"
else
    print_status "WARNING" "Only $TEST_FILES test files found (should have more)"
fi

echo ""
echo "10. Checking Documentation..."
echo "============================="
DOC_COMMENTS=$(grep -r "///" crates --include="*.rs" | wc -l)
if [ "$DOC_COMMENTS" -gt 1000 ]; then
    print_status "SUCCESS" "Found $DOC_COMMENTS documentation comments"
else
    print_status "WARNING" "Only $DOC_COMMENTS documentation comments (needs more)"
fi

echo ""
echo "11. Checking Serialization Consistency..."
echo "========================================="
# Check if Serializable trait is used consistently
SERIALIZABLE_IMPLS=$(grep -r "impl.*Serializable for" crates --include="*.rs" | wc -l)
print_status "SUCCESS" "Found $SERIALIZABLE_IMPLS Serializable implementations"

echo ""
echo "12. Checking Consensus Integration..."
echo "====================================="
if [ -f "crates/consensus/src/extensible_wrapper.rs" ]; then
    print_status "SUCCESS" "Consensus ExtensiblePayload wrapper exists"
else
    print_status "ERROR" "Consensus ExtensiblePayload wrapper missing"
fi

echo ""
echo "==============================================="
echo "                 SUMMARY REPORT                "
echo "==============================================="
echo -e "${GREEN}Success:${NC} $SUCCESS checks passed"
echo -e "${YELLOW}Warnings:${NC} $WARNINGS issues found (non-critical)"
echo -e "${RED}Errors:${NC} $ERRORS critical issues found"
echo ""

if [ "$ERRORS" -eq 0 ]; then
    echo -e "${GREEN}✓ OVERALL STATUS: Implementation is consistent and ready${NC}"
    exit 0
else
    echo -e "${RED}✗ OVERALL STATUS: Critical issues need to be fixed${NC}"
    exit 1
fi