#!/usr/bin/env python3
"""
Comprehensive C# to Rust test converter for Neo project
"""

import os
import re
import sys
from pathlib import Path
from typing import List, Dict, Tuple

class NeoTestConverter:
    def __init__(self):
        self.type_mappings = {
            'UInt160': 'UInt160',
            'UInt256': 'UInt256', 
            'BigDecimal': 'BigDecimal',
            'BigInteger': 'BigInt',
            'byte[]': '&[u8]',
            'string': '&str',
            'bool': 'bool',
            'int': 'i32',
            'uint': 'u32',
            'long': 'i64',
            'ulong': 'u64',
        }
        
        self.assertion_mappings = [
            (r'Assert\.IsTrue\(([^)]+)\)', r'assert!(\1)'),
            (r'Assert\.IsFalse\(([^)]+)\)', r'assert!(!\1)'),
            (r'Assert\.AreEqual\(([^,]+),\s*([^)]+)\)', r'assert_eq!(\1, \2)'),
            (r'Assert\.AreNotEqual\(([^,]+),\s*([^)]+)\)', r'assert_ne!(\1, \2)'),
            (r'Assert\.IsNull\(([^)]+)\)', r'assert!(\1.is_none())'),
            (r'Assert\.IsNotNull\(([^)]+)\)', r'assert!(\1.is_some())'),
            (r'Assert\.ThrowsExactly<([^>]+)>\([^)]+\)', r'assert!(result.is_err())'),
        ]
        
        self.method_mappings = [
            (r'\.ToString\(\)', r'.to_string()'),
            (r'\.Length', r'.len()'),
            (r'\.CompareTo\(([^)]+)\)', r'.cmp(&\1)'),
            (r'\.Equals\(([^)]+)\)', r' == \1'),
            (r'new (\w+)\(\)', r'\1::new()'),
            (r'new byte\[(\d+)\]', r'vec![0u8; \1]'),
        ]

    def extract_test_methods(self, content: str) -> List[Tuple[str, str]]:
        """Extract test methods from C# content"""
        pattern = r'\[TestMethod\]\s*public void (\w+)\(\)[^{]*\{([^}]*(?:\{[^}]*\}[^}]*)*)\}'
        return re.findall(pattern, content, re.DOTALL)

    def convert_method_body(self, body: str) -> str:
        """Convert C# method body to Rust"""
        result = body.strip()
        
        # Apply assertion mappings
        for pattern, replacement in self.assertion_mappings:
            result = re.sub(pattern, replacement, result)
            
        # Apply method mappings
        for pattern, replacement in self.method_mappings:
            result = re.sub(pattern, replacement, result)
            
        # Convert variable declarations
        result = re.sub(r'var (\w+) = ', r'let \1 = ', result)
        result = re.sub(r'(\w+) (\w+) = ', r'let \2: \1 = ', result)
        
        # Convert null to None
        result = re.sub(r'\bnull\b', 'None', result)
        
        # Convert new expressions
        result = re.sub(r'new (\w+)\(([^)]*)\)', r'\1::new(\2)', result)
        
        return result

    def generate_rust_test_file(self, class_name: str, test_methods: List[Tuple[str, str]], 
                               source_file: str, imports: List[str] = None) -> str:
        """Generate complete Rust test file"""
        
        if imports is None:
            imports = [
                'use neo_primitives::{UInt160, UInt256};',
                'use neo_core::*;',
                'use num_bigint::BigInt;'
            ]
        
        rust_content = f"""// Converted from {source_file}
{chr(10).join(imports)}

#[cfg(test)]
mod {class_name.lower()}_tests {{
    use super::*;

"""
        
        for method_name, method_body in test_methods:
            rust_method = self.convert_method_body(method_body)
            rust_content += f"""    #[test]
    fn {method_name.lower()}() {{
        {rust_method}
    }}

"""
        
        rust_content += "}\n"
        return rust_content

    def convert_file(self, csharp_file: Path, rust_file: Path, imports: List[str] = None):
        """Convert a single C# test file to Rust"""
        try:
            with open(csharp_file, 'r', encoding='utf-8') as f:
                content = f.read()
            
            # Extract class name
            class_match = re.search(r'public class (\w+)', content)
            if not class_match:
                print(f"Warning: Could not find class name in {csharp_file}")
                return False
                
            class_name = class_match.group(1)
            
            # Extract test methods
            test_methods = self.extract_test_methods(content)
            if not test_methods:
                print(f"Warning: No test methods found in {csharp_file}")
                return False
            
            # Generate Rust content
            rust_content = self.generate_rust_test_file(
                class_name, test_methods, str(csharp_file), imports
            )
            
            # Write Rust file
            rust_file.parent.mkdir(parents=True, exist_ok=True)
            with open(rust_file, 'w', encoding='utf-8') as f:
                f.write(rust_content)
                
            print(f"✅ Converted {csharp_file.name} -> {rust_file.name} ({len(test_methods)} tests)")
            return True
            
        except Exception as e:
            print(f"❌ Error converting {csharp_file}: {e}")
            return False

def main():
    converter = NeoTestConverter()
    
    csharp_root = Path("/home/neo/git/neo/tests")
    rust_root = Path("/home/neo/git/neo-rs")
    
    # Define conversion mappings
    conversions = [
        # Primitives
        (csharp_root / "Neo.UnitTests/UT_BigDecimal.cs", 
         rust_root / "neo-core/src/tests/big_decimal_tests.rs",
         ['use neo_core::big_decimal::BigDecimal;', 'use num_bigint::BigInt;']),
        
        # VM Tests
        (csharp_root / "Neo.VM.Tests/UT_ExecutionEngine.cs",
         rust_root / "neo-vm/src/tests/csharp_ported/execution_engine_tests.rs",
         ['use neo_vm::*;']),
         
        # SmartContract Tests
        (csharp_root / "Neo.UnitTests/SmartContract/UT_Contract.cs",
         rust_root / "neo-core/src/tests/smart_contract/contract_tests.rs",
         ['use neo_core::smart_contract::*;']),
         
        # Network Tests
        (csharp_root / "Neo.UnitTests/Network/P2P/UT_Message.cs",
         rust_root / "neo-core/src/tests/network/message_tests.rs",
         ['use neo_core::network::*;']),
         
        # Ledger Tests
        (csharp_root / "Neo.UnitTests/Ledger/UT_Blockchain.cs",
         rust_root / "neo-core/src/tests/ledger/blockchain_tests.rs",
         ['use neo_core::ledger::*;']),
         
        # Cryptography Tests
        (csharp_root / "Neo.UnitTests/Cryptography/UT_Crypto.cs",
         rust_root / "neo-crypto/src/tests/crypto_tests.rs",
         ['use neo_crypto::*;']),
    ]
    
    print("🚀 Starting Neo C# to Rust test conversion...")
    
    successful = 0
    total = 0
    
    for csharp_file, rust_file, imports in conversions:
        total += 1
        if csharp_file.exists():
            if converter.convert_file(csharp_file, rust_file, imports):
                successful += 1
        else:
            print(f"⚠️  C# file not found: {csharp_file}")
    
    print(f"\n📊 Conversion Summary: {successful}/{total} files converted successfully")
    
    if successful > 0:
        print("\n📋 Next Steps:")
        print("1. Run 'cargo test --workspace' to check compilation")
        print("2. Fix any compilation errors in converted tests")
        print("3. Manually review and adjust complex test logic")
        print("4. Add missing imports and dependencies")

if __name__ == "__main__":
    main()
