#!/usr/bin/env python3
"""Check for real wildcard imports, excluding false positives."""

import os
import re
import glob

def check_wildcard_imports(file_path):
    """Check for actual wildcard imports in code."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            lines = f.readlines()
        
        wildcards = []
        in_test_block = False
        
        for i, line in enumerate(lines):
            # Check if we're entering a test block
            if '#[cfg(test)]' in line or '#[test]' in line:
                in_test_block = True
            elif line.strip() and not line.strip().startswith('//') and not line.strip().startswith('#'):
                # Reset test block on non-comment, non-attribute line
                if in_test_block and not line.startswith(' '):
                    in_test_block = False
            
            # Skip if in test block
            if in_test_block:
                continue
            
            # Skip doc comments
            if line.strip().startswith('//!') or line.strip().startswith('///'):
                continue
            
            # Check for wildcard import
            if re.search(r'use\s+.*::\*;', line):
                # Skip if it's in a comment
                if '//' in line and line.index('//') < line.index('use'):
                    continue
                
                wildcards.append((i + 1, line.strip()))
        
        return wildcards
    
    except Exception as e:
        print(f"Error checking {file_path}: {e}")
        return []

def main():
    """Main function."""
    total_wildcards = 0
    
    for pattern in ['crates/**/*.rs', 'node/**/*.rs']:
        for file_path in glob.glob(pattern, recursive=True):
            if os.path.isfile(file_path):
                wildcards = check_wildcard_imports(file_path)
                if wildcards:
                    print(f"\n{file_path}:")
                    for line_num, line in wildcards:
                        print(f"  Line {line_num}: {line}")
                    total_wildcards += len(wildcards)
    
    print(f"\nTotal real wildcard imports found: {total_wildcards}")

if __name__ == '__main__':
    main()