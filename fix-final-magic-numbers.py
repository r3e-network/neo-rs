#!/usr/bin/env python3
"""Fix final remaining magic numbers."""

import os
import re
import glob

def fix_final_magic_numbers(file_path):
    """Fix final magic numbers in a file."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        original_content = content
        changes_made = 0
        
        # Skip test files
        if any(skip in file_path for skip in ['test', '/tests/', '/examples/', 'bench']):
            return 0
        
        # Fix specific cases
        
        # 1. Fix 15.0 in average_block_time context
        if 'average_block_time' in content and '15.0' in content:
            content = content.replace('15.0', 'SECONDS_PER_BLOCK as f64')
            changes_made += 1
        
        # 2. Fix 102400i64 in transaction size context
        if '102400i64' in content:
            # Check if MAX_TRANSACTION_SIZE is imported
            if 'MAX_TRANSACTION_SIZE' not in content:
                # Add import
                lines = content.splitlines()
                import_added = False
                for i, line in enumerate(lines):
                    if line.startswith('use ') and 'neo_config' in line:
                        # Add to existing neo_config import
                        if 'MAX_TRANSACTION_SIZE' not in line:
                            lines[i] = line.rstrip(';') + ', MAX_TRANSACTION_SIZE};'
                            import_added = True
                            break
                    elif line.startswith('use '):
                        last_use = i
                
                if not import_added and 'last_use' in locals():
                    lines.insert(last_use + 1, 'use neo_config::MAX_TRANSACTION_SIZE;')
                
                content = '\n'.join(lines)
            
            # Replace the magic number
            content = content.replace('102400i64', 'MAX_TRANSACTION_SIZE as i64')
            changes_made += 1
        
        # 3. Fix remaining bare 15 in block time contexts
        # Look for patterns like "block_time", "interval", etc.
        if re.search(r'\b15\b', content):
            lines = content.splitlines()
            new_lines = []
            
            for line in lines:
                if '15' in line and any(ctx in line.lower() for ctx in ['block', 'time', 'interval', 'seconds']):
                    # This is likely a block time reference
                    line = re.sub(r'\b15\b', 'SECONDS_PER_BLOCK', line)
                    changes_made += 1
                new_lines.append(line)
            
            content = '\n'.join(new_lines)
        
        if content != original_content:
            with open(file_path, 'w', encoding='utf-8') as f:
                f.write(content)
            return changes_made
        
        return 0
    
    except Exception as e:
        print(f"Error processing {file_path}: {e}")
        return 0

def main():
    """Main function to fix final magic numbers."""
    total_fixes = 0
    files_fixed = 0
    
    # Process specific files we know have issues
    target_files = [
        'crates/ledger/src/lib.rs',
        'crates/consensus/src/proposal.rs',
    ]
    
    for file_path in target_files:
        if os.path.exists(file_path):
            fixes = fix_final_magic_numbers(file_path)
            if fixes > 0:
                print(f"Fixed {fixes} magic numbers in {file_path}")
                total_fixes += fixes
                files_fixed += 1
    
    # Also scan all files for any remaining instances
    for pattern in ['crates/**/*.rs', 'node/**/*.rs']:
        for file_path in glob.glob(pattern, recursive=True):
            if os.path.isfile(file_path) and file_path not in target_files:
                fixes = fix_final_magic_numbers(file_path)
                if fixes > 0:
                    print(f"Fixed {fixes} magic numbers in {file_path}")
                    total_fixes += fixes
                    files_fixed += 1
    
    print(f"\nTotal magic numbers fixed: {total_fixes} in {files_fixed} files")

if __name__ == '__main__':
    main()