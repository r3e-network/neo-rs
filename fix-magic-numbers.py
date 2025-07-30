#!/usr/bin/env python3
"""Fix magic numbers by replacing them with named constants."""

import os
import re
import glob

def fix_magic_numbers_in_file(file_path):
    """Fix magic numbers in a single file."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        original_content = content
        changes_made = 0
        
        # Skip if this file already imports config constants
        if 'neo_config::' in content or 'use neo_config' in content:
            print(f"Skipping {file_path} (already uses config constants)")
            return 0
        
        # Fix magic number 15000 (15 seconds in milliseconds) 
        if re.search(r'\b15000\b', content) and 'test' not in file_path:
            if 'use ' in content and not content.startswith('use neo_config'):
                # Add import after existing use statements
                content = re.sub(
                    r'(use [^;]+;)(\s*\n)',
                    r'\1\nuse neo_config::MILLISECONDS_PER_BLOCK;\2',
                    content,
                    count=1
                )
                content = re.sub(r'\b15000\b', 'MILLISECONDS_PER_BLOCK', content)
                changes_made += len(re.findall(r'\b15000\b', original_content))
        
        # Fix magic number 1048576 (1MB)
        if re.search(r'\b1048576\b', content) and 'test' not in file_path:
            if 'use ' in content:
                if 'neo_config::' not in content:
                    content = re.sub(
                        r'(use [^;]+;)(\s*\n)',
                        r'\1\nuse neo_config::MAX_BLOCK_SIZE;\2',
                        content,
                        count=1
                    )
                content = re.sub(r'\b1048576\b', 'MAX_BLOCK_SIZE', content)
                changes_made += len(re.findall(r'\b1048576\b', original_content))
        
        # Fix magic number 102400 (100KB)
        if re.search(r'\b102400\b', content) and 'test' not in file_path:
            if 'use ' in content:
                if 'neo_config::' not in content:
                    content = re.sub(
                        r'(use [^;]+;)(\s*\n)',
                        r'\1\nuse neo_config::MAX_TRANSACTION_SIZE;\2',
                        content,
                        count=1
                    )
                content = re.sub(r'\b102400\b', 'MAX_TRANSACTION_SIZE', content)
                changes_made += len(re.findall(r'\b102400\b', original_content))
        
        # Fix magic number 512 (max transactions per block)
        if re.search(r'\b512\b', content) and 'test' not in file_path and 'max_transactions' in content:
            if 'use ' in content:
                if 'neo_config::' not in content:
                    content = re.sub(
                        r'(use [^;]+;)(\s*\n)',
                        r'\1\nuse neo_config::MAX_TRANSACTIONS_PER_BLOCK;\2',
                        content,
                        count=1
                    )
                content = re.sub(r'\b512\b', 'MAX_TRANSACTIONS_PER_BLOCK', content)
                changes_made += len(re.findall(r'\b512\b', original_content))
        
        if content != original_content:
            with open(file_path, 'w', encoding='utf-8') as f:
                f.write(content)
            print(f"Fixed {changes_made} magic numbers in {file_path}")
            return changes_made
        
        return 0
    
    except Exception as e:
        print(f"Error processing {file_path}: {e}")
        return 0

def main():
    """Main function to fix magic numbers in all Rust files."""
    total_fixes = 0
    
    # Process all Rust files in crates and node directories
    for pattern in ['crates/**/*.rs', 'node/**/*.rs']:
        for file_path in glob.glob(pattern, recursive=True):
            if os.path.isfile(file_path):
                fixes = fix_magic_numbers_in_file(file_path)
                total_fixes += fixes
    
    print(f"\nTotal magic numbers fixed: {total_fixes}")

if __name__ == '__main__':
    main()