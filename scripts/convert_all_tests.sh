#!/bin/bash

# Neo C# to Rust Test Conversion Script
set -e

CSHARP_ROOT="/home/neo/git/neo/tests"
RUST_ROOT="/home/neo/git/neo-rs"

echo "Starting Neo C# to Rust test conversion..."

# Create test directories
mkdir -p "$RUST_ROOT/neo-primitives/src/tests"
mkdir -p "$RUST_ROOT/neo-core/src/tests"
mkdir -p "$RUST_ROOT/neo-vm/src/tests/csharp_ported"
mkdir -p "$RUST_ROOT/neo-rpc/src/tests"
mkdir -p "$RUST_ROOT/neo-consensus/src/tests"

# Function to convert a single test file
convert_test_file() {
    local csharp_file="$1"
    local rust_target="$2"
    local module_name="$3"
    
    echo "Converting $csharp_file -> $rust_target"
    
    # Extract test methods and convert basic patterns
    python3 - << EOF
import re
import sys

def convert_csharp_to_rust(content):
    # Basic type conversions
    conversions = [
        (r'\[TestMethod\]', '#[test]'),
        (r'Assert\.IsTrue\(([^)]+)\)', r'assert!(\1)'),
        (r'Assert\.IsFalse\(([^)]+)\)', r'assert!(!\1)'),
        (r'Assert\.AreEqual\(([^,]+),\s*([^)]+)\)', r'assert_eq!(\1, \2)'),
        (r'Assert\.AreNotEqual\(([^,]+),\s*([^)]+)\)', r'assert_ne!(\1, \2)'),
        (r'Assert\.IsNull\(([^)]+)\)', r'assert!(\1.is_none())'),
        (r'Assert\.IsNotNull\(([^)]+)\)', r'assert!(\1.is_some())'),
        (r'Assert\.ThrowsExactly<FormatException>\([^)]+\)', r'assert!(result.is_err())'),
        (r'new UInt160\(\)', r'UInt160::new()'),
        (r'new UInt256\(\)', r'UInt256::new()'),
        (r'UInt160\.Zero', r'UInt160::zero()'),
        (r'UInt256\.Zero', r'UInt256::zero()'),
        (r'\.ToString\(\)', r'.to_string()'),
        (r'\.Length', r'.len()'),
        (r'new byte\[(\d+)\]', r'vec![0u8; \1]'),
        (r'var (\w+) = ', r'let \1 = '),
        (r'public void (\w+)\(\)', r'fn \1()'),
    ]
    
    result = content
    for pattern, replacement in conversions:
        result = re.sub(pattern, replacement, result)
    
    return result

with open('$csharp_file', 'r') as f:
    content = f.read()

# Extract test methods
test_methods = re.findall(r'\[TestMethod\]\s*public void (\w+)\(\)[^{]*\{([^}]*(?:\{[^}]*\}[^}]*)*)\}', content, re.DOTALL)

rust_content = f"""// Converted from {sys.argv[1]}
use neo_primitives::{{UInt160, UInt256}};
use neo_core::*;

#[cfg(test)]
mod $module_name {{
    use super::*;

"""

for method_name, method_body in test_methods:
    rust_method = convert_csharp_to_rust(method_body.strip())
    rust_content += f"""    #[test]
    fn {method_name.lower()}() {{
        {rust_method}
    }}

"""

rust_content += "}\n"

with open('$rust_target', 'w') as f:
    f.write(rust_content)

EOF
}

# Convert core primitive tests
echo "Converting UInt160 tests..."
if [ -f "$CSHARP_ROOT/Neo.UnitTests/UT_UInt160.cs" ]; then
    convert_test_file "$CSHARP_ROOT/Neo.UnitTests/UT_UInt160.cs" "$RUST_ROOT/neo-primitives/src/tests/uint160_tests.rs" "uint160_tests"
fi

echo "Converting UInt256 tests..."
if [ -f "$CSHARP_ROOT/Neo.UnitTests/UT_UInt256.cs" ]; then
    convert_test_file "$CSHARP_ROOT/Neo.UnitTests/UT_UInt256.cs" "$RUST_ROOT/neo-primitives/src/tests/uint256_tests.rs" "uint256_tests"
fi

echo "Converting BigDecimal tests..."
if [ -f "$CSHARP_ROOT/Neo.UnitTests/UT_BigDecimal.cs" ]; then
    convert_test_file "$CSHARP_ROOT/Neo.UnitTests/UT_BigDecimal.cs" "$RUST_ROOT/neo-core/src/tests/big_decimal_tests.rs" "big_decimal_tests"
fi

# Convert VM tests
echo "Converting VM tests..."
if [ -d "$CSHARP_ROOT/Neo.VM.Tests" ]; then
    for vm_test in "$CSHARP_ROOT/Neo.VM.Tests"/*.cs; do
        if [ -f "$vm_test" ]; then
            basename=$(basename "$vm_test" .cs)
            convert_test_file "$vm_test" "$RUST_ROOT/neo-vm/src/tests/csharp_ported/${basename,,}.rs" "${basename,,}_tests"
        fi
    done
fi

# Convert core Neo tests
echo "Converting core Neo tests..."
for core_test in "$CSHARP_ROOT/Neo.UnitTests"/UT_*.cs; do
    if [ -f "$core_test" ]; then
        basename=$(basename "$core_test" .cs)
        convert_test_file "$core_test" "$RUST_ROOT/neo-core/src/tests/${basename,,}.rs" "${basename,,}_tests"
    fi
done

# Convert SmartContract tests
echo "Converting SmartContract tests..."
if [ -d "$CSHARP_ROOT/Neo.UnitTests/SmartContract" ]; then
    mkdir -p "$RUST_ROOT/neo-core/src/tests/smart_contract"
    for sc_test in "$CSHARP_ROOT/Neo.UnitTests/SmartContract"/*.cs; do
        if [ -f "$sc_test" ]; then
            basename=$(basename "$sc_test" .cs)
            convert_test_file "$sc_test" "$RUST_ROOT/neo-core/src/tests/smart_contract/${basename,,}.rs" "${basename,,}_tests"
        fi
    done
fi

# Convert Network tests
echo "Converting Network tests..."
if [ -d "$CSHARP_ROOT/Neo.UnitTests/Network" ]; then
    mkdir -p "$RUST_ROOT/neo-core/src/tests/network"
    for net_test in "$CSHARP_ROOT/Neo.UnitTests/Network"/*.cs; do
        if [ -f "$net_test" ]; then
            basename=$(basename "$net_test" .cs)
            convert_test_file "$net_test" "$RUST_ROOT/neo-core/src/tests/network/${basename,,}.rs" "${basename,,}_tests"
        fi
    done
fi

# Convert Cryptography tests
echo "Converting Cryptography tests..."
if [ -d "$CSHARP_ROOT/Neo.UnitTests/Cryptography" ]; then
    mkdir -p "$RUST_ROOT/neo-crypto/src/tests"
    for crypto_test in "$CSHARP_ROOT/Neo.UnitTests/Cryptography"/*.cs; do
        if [ -f "$crypto_test" ]; then
            basename=$(basename "$crypto_test" .cs)
            convert_test_file "$crypto_test" "$RUST_ROOT/neo-crypto/src/tests/${basename,,}.rs" "${basename,,}_tests"
        fi
    done
fi

# Update module declarations
echo "Updating module declarations..."

# Add tests to neo-primitives
cat >> "$RUST_ROOT/neo-primitives/src/tests/mod.rs" << 'EOF'
pub mod uint256_tests;
EOF

# Add tests to neo-core
cat > "$RUST_ROOT/neo-core/src/tests/mod.rs" << 'EOF'
pub mod big_decimal_tests;
pub mod smart_contract;
pub mod network;

// Re-export all test modules
pub use big_decimal_tests::*;
EOF

# Add tests to neo-crypto
cat > "$RUST_ROOT/neo-crypto/src/tests/mod.rs" << 'EOF'
// Cryptography tests converted from C#
EOF

# Add tests to neo-vm
cat > "$RUST_ROOT/neo-vm/src/tests/csharp_ported/mod.rs" << 'EOF'
// VM tests converted from C#
EOF

echo "Test conversion completed!"
echo "Next steps:"
echo "1. Review converted tests for syntax errors"
echo "2. Fix import statements and type references"
echo "3. Run 'cargo test' to identify compilation issues"
echo "4. Manually fix complex test logic that couldn't be auto-converted"
