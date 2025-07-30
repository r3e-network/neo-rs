#!/usr/bin/env python3
"""Fix unwrap() calls in blockchain/ledger modules."""

import os
import re
import glob

def fix_blockchain_unwraps(file_path):
    """Fix unwraps in blockchain module files."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        original_content = content
        changes_made = 0
        
        # Skip test files
        if 'test' in file_path or '/tests/' in file_path:
            return 0
        
        # Fix UInt160::from_bytes(&[]).unwrap()
        pattern1 = r'UInt160::from_bytes\(&\[[^\]]+\]\)\.unwrap\(\)'
        if re.search(pattern1, content):
            content = re.sub(
                pattern1,
                lambda m: m.group(0).replace('.unwrap()', '.expect("Fixed-size array should be valid UInt160")'),
                content
            )
            changes_made += len(re.findall(pattern1, original_content))
        
        # Fix UInt256::from_bytes(&[]).unwrap()
        pattern2 = r'UInt256::from_bytes\(&\[[^\]]+\]\)\.unwrap\(\)'
        if re.search(pattern2, content):
            content = re.sub(
                pattern2,
                lambda m: m.group(0).replace('.unwrap()', '.expect("Fixed-size array should be valid UInt256")'),
                content
            )
            changes_made += len(re.findall(pattern2, original_content))
        
        # Fix .build().unwrap() for builders
        pattern3 = r'\.build\(\)\.unwrap\(\)'
        if re.search(pattern3, content):
            content = re.sub(
                pattern3,
                '.build().expect("Block builder should succeed with valid inputs")',
                content
            )
            changes_made += len(re.findall(pattern3, original_content))
        
        # Fix .get().unwrap() patterns
        pattern4 = r'\.get\([^)]+\)\.unwrap\(\)'
        if re.search(pattern4, content):
            content = re.sub(
                pattern4,
                lambda m: m.group(0).replace('.unwrap()', '.expect("Key should exist")'),
                content
            )
            changes_made += len(re.findall(pattern4, original_content))
        
        # Fix .remove().unwrap() patterns
        pattern5 = r'\.remove\([^)]+\)\.unwrap\(\)'
        if re.search(pattern5, content):
            content = re.sub(
                pattern5,
                lambda m: m.group(0).replace('.unwrap()', '.expect("Key should exist for removal")'),
                content
            )
            changes_made += len(re.findall(pattern5, original_content))
        
        # Fix .header().unwrap() patterns
        pattern6 = r'\.header\(\)\.unwrap\(\)'
        if re.search(pattern6, content):
            content = re.sub(
                pattern6,
                '.header().expect("Block should have header")',
                content
            )
            changes_made += len(re.findall(pattern6, original_content))
        
        # Fix .pop().unwrap() for transaction pools
        pattern7 = r'\.pop\(\)\.unwrap\(\)'
        if re.search(pattern7, content):
            content = re.sub(
                pattern7,
                '.pop().expect("Should have transaction to pop")',
                content
            )
            changes_made += len(re.findall(pattern7, original_content))
        
        if content != original_content:
            with open(file_path, 'w', encoding='utf-8') as f:
                f.write(content)
            print(f"Fixed {changes_made} unwraps in {file_path}")
            return changes_made
        
        return 0
    
    except Exception as e:
        print(f"Error processing {file_path}: {e}")
        return 0

def main():
    """Main function to fix blockchain unwraps."""
    total_fixes = 0
    
    # Process ledger and blockchain module files
    for pattern in ['crates/ledger/src/**/*.rs', 'crates/core/src/transaction/**/*.rs']:
        for file_path in glob.glob(pattern, recursive=True):
            if os.path.isfile(file_path):
                fixes = fix_blockchain_unwraps(file_path)
                total_fixes += fixes
    
    print(f"\nTotal blockchain unwraps fixed: {total_fixes}")

if __name__ == '__main__':
    main()