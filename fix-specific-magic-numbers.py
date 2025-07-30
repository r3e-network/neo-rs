#!/usr/bin/env python3
"""Fix specific remaining magic numbers."""

import os
import re
import glob

def fix_specific_magic_numbers(file_path):
    """Fix specific magic numbers that were missed."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        original_content = content
        changes_made = 0
        
        # Skip test files
        if any(skip in file_path for skip in ['test', '/tests/', '/examples/', 'bench']):
            return 0
        
        # Fix 102400 (MAX_TRANSACTION_SIZE)
        # But be careful not to change 2102400 (different constant)
        pattern1 = r'\b102400\b(?![\d])'
        if re.search(pattern1, content):
            # Check if MAX_TRANSACTION_SIZE is imported
            if 'MAX_TRANSACTION_SIZE' not in content:
                # Add import if needed
                if 'use neo_config::' in content:
                    # Add to existing import
                    content = re.sub(
                        r'(use neo_config::\{)([^}]+)(\};)',
                        lambda m: f"{m.group(1)}{m.group(2)}, MAX_TRANSACTION_SIZE{m.group(3)}" if 'MAX_TRANSACTION_SIZE' not in m.group(2) else m.group(0),
                        content
                    )
                else:
                    # Add new import after other use statements
                    lines = content.splitlines()
                    import_idx = None
                    for i, line in enumerate(lines):
                        if line.startswith('use '):
                            import_idx = i
                    
                    if import_idx is not None:
                        lines.insert(import_idx + 1, 'use neo_config::MAX_TRANSACTION_SIZE;')
                        content = '\n'.join(lines)
            
            # Replace the magic number
            content = re.sub(pattern1, 'MAX_TRANSACTION_SIZE', content)
            changes_made += len(re.findall(pattern1, original_content))
        
        # Fix 2102400 (MAX_TRACEABLE_BLOCKS)
        pattern2 = r'\b2102400\b'
        if re.search(pattern2, content):
            # This is a different constant - MAX_TRACEABLE_BLOCKS
            # Add to config if not there
            if 'policy_contract.rs' in file_path:
                # Skip - this file defines the constant
                pass
            else:
                content = re.sub(pattern2, 'MAX_TRACEABLE_BLOCKS', content)
                changes_made += len(re.findall(pattern2, original_content))
        
        # Fix remaining 15 (SECONDS_PER_BLOCK)
        pattern3 = r'\b15\b(?![\d\.])'
        # Only in specific contexts
        time_contexts = ['seconds', 'timeout', 'interval', 'duration', 'time', 'block']
        for match in re.finditer(pattern3, content):
            # Check context around the match
            start = max(0, match.start() - 50)
            end = min(len(content), match.end() + 50)
            context = content[start:end].lower()
            
            if any(ctx in context for ctx in time_contexts):
                # Replace with SECONDS_PER_BLOCK
                content = content[:match.start()] + 'SECONDS_PER_BLOCK' + content[match.end():]
                changes_made += 1
        
        if content != original_content:
            with open(file_path, 'w', encoding='utf-8') as f:
                f.write(content)
            print(f"Fixed {changes_made} magic numbers in {file_path}")
            return changes_made
        
        return 0
    
    except Exception as e:
        print(f"Error processing {file_path}: {e}")
        return 0

def add_missing_constants():
    """Add MAX_TRACEABLE_BLOCKS to config if missing."""
    config_path = '/Users/jinghuiliao/git/r3e/neo-rs/crates/config/src/lib.rs'
    
    try:
        with open(config_path, 'r') as f:
            content = f.read()
        
        if 'MAX_TRACEABLE_BLOCKS' not in content:
            # Add the constant
            lines = content.splitlines()
            
            # Find where to add
            for i, line in enumerate(lines):
                if 'pub const MAX_TRANSACTIONS_PER_BLOCK' in line:
                    lines.insert(i + 1, '')
                    lines.insert(i + 2, '/// Maximum number of blocks that can be traced (about 1 year)')
                    lines.insert(i + 3, 'pub const MAX_TRACEABLE_BLOCKS: u32 = 2_102_400;')
                    break
            
            with open(config_path, 'w') as f:
                f.write('\n'.join(lines))
            
            print("Added MAX_TRACEABLE_BLOCKS to config")
    except Exception as e:
        print(f"Error updating config: {e}")

def main():
    """Main function to fix specific magic numbers."""
    # First add missing constants
    add_missing_constants()
    
    total_fixes = 0
    
    # Process all Rust files
    for pattern in ['crates/**/*.rs', 'node/**/*.rs']:
        for file_path in glob.glob(pattern, recursive=True):
            if os.path.isfile(file_path):
                fixes = fix_specific_magic_numbers(file_path)
                total_fixes += fixes
    
    print(f"\nTotal specific magic numbers fixed: {total_fixes}")

if __name__ == '__main__':
    main()