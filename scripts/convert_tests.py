#!/usr/bin/env python3
"""
Convert C# Neo tests to Rust tests
"""
import os
import re
import sys
from pathlib import Path

class CSharpToRustTestConverter:
    def __init__(self, csharp_root, rust_root):
        self.csharp_root = Path(csharp_root)
        self.rust_root = Path(rust_root)
        
    def convert_test_method(self, csharp_code):
        """Convert a single C# test method to Rust"""
        # Basic conversions
        conversions = [
            # Test attributes
            (r'\[TestMethod\]', '#[test]'),
            (r'\[TestClass\]', ''),
            
            # Assertions
            (r'Assert\.IsTrue\(([^)]+)\)', r'assert!(\1)'),
            (r'Assert\.IsFalse\(([^)]+)\)', r'assert!(!\1)'),
            (r'Assert\.AreEqual\(([^,]+),\s*([^)]+)\)', r'assert_eq!(\1, \2)'),
            (r'Assert\.AreNotEqual\(([^,]+),\s*([^)]+)\)', r'assert_ne!(\1, \2)'),
            (r'Assert\.IsNull\(([^)]+)\)', r'assert!(\1.is_none())'),
            (r'Assert\.IsNotNull\(([^)]+)\)', r'assert!(\1.is_some())'),
            (r'Assert\.ThrowsExactly<([^>]+)>\(([^)]+)\)', r'assert!(std::panic::catch_unwind(|| \2).is_err())'),
            
            # Types
            (r'\bUInt160\b', 'UInt160'),
            (r'\bUInt256\b', 'UInt256'),
            (r'\bbyte\[\]', '&[u8]'),
            (r'\bnew byte\[(\d+)\]', r'vec![0u8; \1]'),
            (r'\.Length', '.len()'),
            
            # Method signatures
            (r'public void (\w+)\(\)', r'fn \1()'),
            (r'public static void (\w+)\(\)', r'fn \1()'),
            
            # String literals
            (r'"0x([0-9a-fA-F]+)"', r'"\1"'),
            
            # Null checks
            (r'\bnull\b', 'None'),
            
            # Variable declarations
            (r'var (\w+) = ', r'let \1 = '),
            (r'UInt160 (\w+) = ', r'let \1: UInt160 = '),
        ]
        
        result = csharp_code
        for pattern, replacement in conversions:
            result = re.sub(pattern, replacement, result)
            
        return result
    
    def convert_file(self, csharp_file_path):
        """Convert a C# test file to Rust"""
        with open(csharp_file_path, 'r') as f:
            content = f.read()
            
        # Extract class name and namespace
        class_match = re.search(r'public class (\w+)', content)
        if not class_match:
            return None
            
        class_name = class_match.group(1)
        
        # Extract test methods
        test_methods = re.findall(r'\[TestMethod\]\s*public void (\w+)\(\)[^{]*\{([^}]*(?:\{[^}]*\}[^}]*)*)\}', content, re.DOTALL)
        
        rust_content = f"""// Converted from {csharp_file_path.name}
use neo_primitives::{{UInt160, UInt256}};

#[cfg(test)]
mod {class_name.lower()}_tests {{
    use super::*;

"""
        
        for method_name, method_body in test_methods:
            rust_method = self.convert_test_method(method_body)
            rust_content += f"""    #[test]
    fn {method_name.lower()}() {{
{rust_method}
    }}

"""
        
        rust_content += "}\n"
        return rust_content

def main():
    if len(sys.argv) != 3:
        print("Usage: python3 convert_tests.py <csharp_tests_dir> <rust_tests_dir>")
        sys.exit(1)
        
    converter = CSharpToRustTestConverter(sys.argv[1], sys.argv[2])
    
    # Find all C# test files
    csharp_test_files = list(Path(sys.argv[1]).rglob("UT_*.cs"))
    
    for csharp_file in csharp_test_files:
        print(f"Converting {csharp_file}")
        rust_content = converter.convert_file(csharp_file)
        if rust_content:
            # Create corresponding Rust file
            relative_path = csharp_file.relative_to(sys.argv[1])
            rust_file = Path(sys.argv[2]) / relative_path.with_suffix('.rs')
            rust_file.parent.mkdir(parents=True, exist_ok=True)
            
            with open(rust_file, 'w') as f:
                f.write(rust_content)
            print(f"Created {rust_file}")

if __name__ == "__main__":
    main()
