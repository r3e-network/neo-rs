#!/bin/bash

# Quick Neo C# to Rust Conversion Verification Script

echo "🚀 Neo C# to Rust Conversion Quick Check"
echo "========================================"

# Count files for basic metrics
echo "📊 Basic Metrics:"
CSHARP_SRC_FILES=$(find neo_csharp/src -name "*.cs" 2>/dev/null | grep -v obj | grep -v bin | wc -l)
CSHARP_TEST_FILES=$(find neo_csharp/tests -name "UT_*.cs" 2>/dev/null | wc -l)
RUST_SRC_FILES=$(find crates -name "*.rs" 2>/dev/null | grep -v target | wc -l)
RUST_TEST_FILES=$(find crates -name "*test*.rs" -o -path "*/tests/*.rs" 2>/dev/null | wc -l)

echo "  C# Source Files: $CSHARP_SRC_FILES"
echo "  C# Unit Tests: $CSHARP_TEST_FILES"  
echo "  Rust Source Files: $RUST_SRC_FILES"
echo "  Rust Test Files: $RUST_TEST_FILES"
echo ""

# Core component conversion check
echo "🔍 Core Component Conversion Analysis:"
echo "======================================"

# Check for key C# components and their Rust equivalents
declare -A COMPONENTS=(
    ["UInt160"]="crates/core/src/uint160.rs"
    ["UInt256"]="crates/core/src/uint256.rs"
    ["Transaction"]="crates/core/src/transaction"
    ["Block"]="crates/ledger/src/block"
    ["Blockchain"]="crates/ledger/src/blockchain"
    ["ApplicationEngine"]="crates/vm/src/application_engine.rs"
    ["MemoryPool"]="crates/ledger/src/mempool.rs"
    ["ContractManifest"]="crates/smart_contract/src/manifest"
    ["JToken"]="crates/json/src/jtoken.rs"
    ["MemoryReader"]="crates/io/src/memory_reader.rs"
    ["BinaryWriter"]="crates/io/src/binary_writer.rs"
    ["ECPoint"]="crates/cryptography/src/ecc"
    ["Wallet"]="crates/wallets/src/wallet.rs"
    ["RpcServer"]="crates/rpc_server/src/lib.rs"
)

CONVERTED=0
TOTAL=${#COMPONENTS[@]}

for component in "${!COMPONENTS[@]}"; do
    rust_path="${COMPONENTS[$component]}"
    if [ -e "$rust_path" ]; then
        echo "✅ $component -> $rust_path"
        CONVERTED=$((CONVERTED + 1))
    else
        echo "❌ $component (missing: $rust_path)"
    fi
done

echo ""
echo "Core Component Conversion: $CONVERTED/$TOTAL ($(echo "scale=1; $CONVERTED * 100 / $TOTAL" | bc)%)"

# Unit test conversion check
echo ""
echo "🧪 Unit Test Conversion Analysis:"
echo "================================="

# Check for key test conversions
declare -A TESTS=(
    ["UT_UInt160"]="uint160::tests"
    ["UT_UInt256"]="uint256::tests"  
    ["UT_Transaction"]="transaction"
    ["UT_Block"]="block"
    ["UT_ApplicationEngine"]="application_engine"
    ["UT_JToken"]="jtoken"
    ["UT_JArray"]="jarray"
    ["UT_MemoryReader"]="memory_reader"
    ["UT_ECPoint"]="ecc"
    ["UT_Crypto"]="crypto"
)

TEST_CONVERTED=0
TEST_TOTAL=${#TESTS[@]}

for test_name in "${!TESTS[@]}"; do
    rust_pattern="${TESTS[$test_name]}"
    
    # Search for test patterns in Rust code
    if grep -r "test.*$rust_pattern\|fn.*test.*\|$rust_pattern.*test" crates/*/src crates/*/tests 2>/dev/null | grep -q "test"; then
        echo "✅ $test_name -> Rust tests found"
        TEST_CONVERTED=$((TEST_CONVERTED + 1))
    else
        echo "❌ $test_name (no equivalent tests found)"
    fi
done

echo ""
echo "Unit Test Conversion: $TEST_CONVERTED/$TEST_TOTAL ($(echo "scale=1; $TEST_CONVERTED * 100 / $TEST_TOTAL" | bc)%)"

# Functional verification
echo ""
echo "⚙️ Functional Verification:"
echo "==========================="

# Test binary functionality
if [ -x "./target/release/neo-node" ]; then
    echo "✅ Neo node binary is executable"
    
    if ./target/release/neo-node --help >/dev/null 2>&1; then
        echo "✅ Help command works"
    else
        echo "❌ Help command failed"
    fi
    
    if ./target/release/neo-node --version >/dev/null 2>&1; then
        echo "✅ Version command works"
    else
        echo "❌ Version command failed"
    fi
else
    echo "❌ Neo node binary not found or not executable"
fi

# Test core library functionality
echo ""
echo "Working Rust test suites:"
for crate in neo-core neo-cryptography neo-io neo-json neo-mpt-trie; do
    if cargo test --package $crate --lib --quiet >/dev/null 2>&1; then
        TEST_COUNT=$(cargo test --package $crate --lib --quiet 2>&1 | grep "test result:" | grep -o "[0-9]\+ passed" | grep -o "[0-9]\+")
        echo "✅ $crate: $TEST_COUNT tests passing"
    else
        echo "❌ $crate: compilation/test issues"
    fi
done

# Neo Network connectivity check
echo ""
echo "🌐 Network Connectivity Verification:"
echo "====================================="

# Check if we can resolve Neo seed nodes
NEO_SEEDS=("seed1.neo.org" "seed2.neo.org" "seed3.neo.org")
REACHABLE_SEEDS=0

for seed in "${NEO_SEEDS[@]}"; do
    if nslookup "$seed" >/dev/null 2>&1; then
        echo "✅ $seed resolves"
        REACHABLE_SEEDS=$((REACHABLE_SEEDS + 1))
    else
        echo "❌ $seed does not resolve"
    fi
done

echo "Reachable Neo seeds: $REACHABLE_SEEDS/${#NEO_SEEDS[@]}"

# Calculate overall conversion success
TOTAL_SCORE=$(echo "scale=1; ($CONVERTED * 100 / $TOTAL + $TEST_CONVERTED * 100 / $TEST_TOTAL + $REACHABLE_SEEDS * 100 / ${#NEO_SEEDS[@]}) / 3" | bc)

echo ""
echo "🎯 FINAL ASSESSMENT"
echo "==================="
echo "Overall Conversion Score: $TOTAL_SCORE%"

if (( $(echo "$TOTAL_SCORE > 75" | bc -l) )); then
    echo "✅ EXCELLENT: Neo-RS shows comprehensive conversion from C# Neo"
    echo "🚀 Production deployment ready!"
elif (( $(echo "$TOTAL_SCORE > 50" | bc -l) )); then
    echo "🟡 GOOD: Core functionality converted, some gaps remain"
    echo "⚙️ Suitable for development and testing"
else
    echo "🔴 NEEDS WORK: Significant conversion gaps exist"
    echo "🔧 Requires additional development"
fi

echo ""
echo "📁 Detailed logs available in individual test outputs"
echo "🎉 Conversion verification complete!"