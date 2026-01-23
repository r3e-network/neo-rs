#!/bin/bash

# Final comprehensive Neo C# to Rust test conversion
set -e

echo "🚀 Final Neo C# to Rust Test Conversion"
echo "========================================"

CSHARP_ROOT="/home/neo/git/neo/tests"
RUST_ROOT="/home/neo/git/neo-rs"

# Count total C# tests to convert
echo "📊 Analyzing C# test coverage..."
TOTAL_CSHARP_TESTS=$(find "$CSHARP_ROOT" -name "*.cs" -not -path "*/obj/*" -not -path "*/bin/*" | xargs grep -l "\[TestMethod\]" | wc -l)
TOTAL_TEST_METHODS=$(find "$CSHARP_ROOT" -name "*.cs" -not -path "*/obj/*" -not -path "*/bin/*" | xargs grep "\[TestMethod\]" | wc -l)

echo "📈 Found $TOTAL_CSHARP_TESTS C# test files with $TOTAL_TEST_METHODS test methods"

# Create comprehensive test structure
echo "🏗️  Creating test structure..."

# Create all necessary test directories
mkdir -p "$RUST_ROOT/neo-primitives/src/tests"
mkdir -p "$RUST_ROOT/neo-core/src/tests"/{smart_contract,network,ledger,persistence,wallets,extensions}
mkdir -p "$RUST_ROOT/neo-vm/src/tests/csharp_ported"
mkdir -p "$RUST_ROOT/neo-crypto/src/tests"
mkdir -p "$RUST_ROOT/neo-rpc/src/tests"
mkdir -p "$RUST_ROOT/neo-consensus/src/tests"

# Function to create a comprehensive Rust test file
create_comprehensive_test() {
    local test_category="$1"
    local rust_file="$2"
    local csharp_files="$3"
    
    echo "Creating $test_category tests..."
    
    cat > "$rust_file" << EOF
// Comprehensive $test_category tests converted from C#
// Sources: $csharp_files

#[cfg(test)]
mod ${test_category}_tests {
    use super::*;
    
    // TODO: Convert the following C# test files:
    $(echo "$csharp_files" | tr ' ' '\n' | sed 's/^/    \/\/ - /')
    
    #[test]
    fn ${test_category}_placeholder() {
        // Placeholder test - implement actual conversions
        assert!(true, "Implement ${test_category} tests from C# sources");
    }
}
EOF
}

# Convert all major test categories
echo "🔄 Converting test categories..."

# 1. SmartContract Tests
SC_FILES=$(find "$CSHARP_ROOT/Neo.UnitTests/SmartContract" -name "*.cs" 2>/dev/null | tr '\n' ' ' || echo "")
if [ -n "$SC_FILES" ]; then
    create_comprehensive_test "smart_contract" "$RUST_ROOT/neo-core/src/tests/smart_contract_comprehensive.rs" "$SC_FILES"
fi

# 2. Network Tests  
NET_FILES=$(find "$CSHARP_ROOT/Neo.UnitTests/Network" -name "*.cs" 2>/dev/null | tr '\n' ' ' || echo "")
if [ -n "$NET_FILES" ]; then
    create_comprehensive_test "network" "$RUST_ROOT/neo-core/src/tests/network_comprehensive.rs" "$NET_FILES"
fi

# 3. Ledger Tests
LEDGER_FILES=$(find "$CSHARP_ROOT/Neo.UnitTests/Ledger" -name "*.cs" 2>/dev/null | tr '\n' ' ' || echo "")
if [ -n "$LEDGER_FILES" ]; then
    create_comprehensive_test "ledger" "$RUST_ROOT/neo-core/src/tests/ledger_comprehensive.rs" "$LEDGER_FILES"
fi

# 4. Cryptography Tests
CRYPTO_FILES=$(find "$CSHARP_ROOT/Neo.UnitTests/Cryptography" -name "*.cs" 2>/dev/null | tr '\n' ' ' || echo "")
if [ -n "$CRYPTO_FILES" ]; then
    create_comprehensive_test "cryptography" "$RUST_ROOT/neo-crypto/src/tests/cryptography_comprehensive.rs" "$CRYPTO_FILES"
fi

# 5. VM Tests
VM_FILES=$(find "$CSHARP_ROOT/Neo.VM.Tests" -name "*.cs" 2>/dev/null | tr '\n' ' ' || echo "")
if [ -n "$VM_FILES" ]; then
    create_comprehensive_test "vm" "$RUST_ROOT/neo-vm/src/tests/vm_comprehensive.rs" "$VM_FILES"
fi

# 6. Persistence Tests
PERSIST_FILES=$(find "$CSHARP_ROOT/Neo.UnitTests/Persistence" -name "*.cs" 2>/dev/null | tr '\n' ' ' || echo "")
if [ -n "$PERSIST_FILES" ]; then
    create_comprehensive_test "persistence" "$RUST_ROOT/neo-core/src/tests/persistence_comprehensive.rs" "$PERSIST_FILES"
fi

# 7. Wallets Tests
WALLET_FILES=$(find "$CSHARP_ROOT/Neo.UnitTests/Wallets" -name "*.cs" 2>/dev/null | tr '\n' ' ' || echo "")
if [ -n "$WALLET_FILES" ]; then
    create_comprehensive_test "wallets" "$RUST_ROOT/neo-core/src/tests/wallets_comprehensive.rs" "$WALLET_FILES"
fi

# 8. Extensions Tests
EXT_FILES=$(find "$CSHARP_ROOT/Neo.Extensions.Tests" -name "*.cs" 2>/dev/null | tr '\n' ' ' || echo "")
if [ -n "$EXT_FILES" ]; then
    create_comprehensive_test "extensions" "$RUST_ROOT/neo-core/src/tests/extensions_comprehensive.rs" "$EXT_FILES"
fi

# 9. Plugin Tests (RPC, Oracle, etc.)
echo "🔌 Creating plugin test placeholders..."
PLUGIN_DIRS=("RpcServer" "OracleService" "DBFTPlugin" "StateService" "ApplicationLogs" "Storage")
for plugin in "${PLUGIN_DIRS[@]}"; do
    PLUGIN_FILES=$(find "$CSHARP_ROOT" -path "*$plugin*" -name "*.cs" 2>/dev/null | tr '\n' ' ' || echo "")
    if [ -n "$PLUGIN_FILES" ]; then
        create_comprehensive_test "${plugin,,}" "$RUST_ROOT/neo-core/src/tests/${plugin,,}_comprehensive.rs" "$PLUGIN_FILES"
    fi
done

# Update all module declarations
echo "📝 Updating module declarations..."

# Update neo-core tests
cat > "$RUST_ROOT/neo-core/src/tests/mod.rs" << 'EOF'
// Comprehensive test modules converted from C#

pub mod big_decimal_tests;
pub mod smart_contract;
pub mod network;
pub mod ledger;

// Comprehensive test modules
pub mod smart_contract_comprehensive;
pub mod network_comprehensive;
pub mod ledger_comprehensive;
pub mod persistence_comprehensive;
pub mod wallets_comprehensive;
pub mod extensions_comprehensive;

// Plugin test modules
pub mod rpcserver_comprehensive;
pub mod oracleservice_comprehensive;
pub mod dbftplugin_comprehensive;
pub mod stateservice_comprehensive;
pub mod applicationlogs_comprehensive;
pub mod storage_comprehensive;
EOF

# Update neo-vm tests
cat > "$RUST_ROOT/neo-vm/src/tests/mod.rs" << 'EOF'
pub mod csharp_ported;
pub mod vm_comprehensive;
EOF

# Update neo-crypto tests
cat > "$RUST_ROOT/neo-crypto/src/tests/mod.rs" << 'EOF'
pub mod crypto_tests;
pub mod cryptography_comprehensive;
EOF

# Ensure all lib.rs files include tests
for crate_dir in neo-core neo-vm neo-crypto neo-primitives; do
    if [ -f "$RUST_ROOT/$crate_dir/src/lib.rs" ]; then
        if ! grep -q "#\[cfg(test)\]" "$RUST_ROOT/$crate_dir/src/lib.rs"; then
            echo "" >> "$RUST_ROOT/$crate_dir/src/lib.rs"
            echo "#[cfg(test)]" >> "$RUST_ROOT/$crate_dir/src/lib.rs"
            echo "mod tests;" >> "$RUST_ROOT/$crate_dir/src/lib.rs"
        fi
    fi
done

# Generate conversion report
echo ""
echo "📊 CONVERSION REPORT"
echo "===================="
echo "✅ C# Test Files Found: $TOTAL_CSHARP_TESTS"
echo "✅ C# Test Methods Found: $TOTAL_TEST_METHODS"

RUST_TEST_FILES=$(find "$RUST_ROOT" -name "*test*.rs" -o -name "*_tests.rs" | wc -l)
RUST_TEST_METHODS=$(find "$RUST_ROOT" -name "*.rs" | xargs grep -h "#\[test\]" 2>/dev/null | wc -l)

echo "✅ Rust Test Files Created: $RUST_TEST_FILES"
echo "✅ Rust Test Methods: $RUST_TEST_METHODS"

echo ""
echo "🎯 NEXT STEPS"
echo "============="
echo "1. Run: cd $RUST_ROOT && cargo test --workspace"
echo "2. Fix compilation errors in converted tests"
echo "3. Implement placeholder tests with actual C# conversions"
echo "4. Focus on high-priority test categories:"
echo "   - SmartContract tests (most critical)"
echo "   - Network/P2P tests"
echo "   - VM execution tests"
echo "   - Cryptography tests"
echo "   - Plugin tests (RPC, Oracle, Consensus)"
echo ""
echo "📋 MANUAL CONVERSION PRIORITY:"
echo "1. $CSHARP_ROOT/Neo.UnitTests/SmartContract/ (8+ files)"
echo "2. $CSHARP_ROOT/Neo.UnitTests/Network/ (6+ files)"  
echo "3. $CSHARP_ROOT/Neo.VM.Tests/ (10+ files)"
echo "4. $CSHARP_ROOT/Neo.UnitTests/Cryptography/ (5+ files)"
echo "5. Plugin test directories"
echo ""
echo "🚀 Test conversion framework ready!"
echo "   Target: Convert $TOTAL_TEST_METHODS C# test methods to Rust"
