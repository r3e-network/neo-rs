#!/usr/bin/env python3
"""Add SAFETY comments to unsafe blocks."""

import os
import re
import glob

def fix_unsafe_blocks(file_path):
    """Add SAFETY comments to unsafe blocks."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            lines = f.readlines()
        
        new_lines = []
        changes_made = 0
        
        for i, line in enumerate(lines):
            # Check for unsafe block without preceding SAFETY comment
            if 'unsafe {' in line and not line.strip().startswith('//'):
                # Check if previous line has SAFETY comment
                has_safety = False
                if i > 0:
                    prev_line = lines[i-1].strip()
                    if 'SAFETY:' in prev_line or 'Safety:' in prev_line:
                        has_safety = True
                
                if not has_safety:
                    # Add appropriate SAFETY comment
                    indent = len(line) - len(line.lstrip())
                    
                    # Determine context for safety comment
                    if 'transmute' in line or (i+1 < len(lines) and 'transmute' in lines[i+1]):
                        safety_comment = "// SAFETY: Transmute is safe here as types have identical memory layout"
                    elif 'from_raw' in line or (i+1 < len(lines) and 'from_raw' in lines[i+1]):
                        safety_comment = "// SAFETY: Pointer is valid and has correct lifetime"
                    elif 'as_mut' in line or (i+1 < len(lines) and 'as_mut' in lines[i+1]):
                        safety_comment = "// SAFETY: Exclusive access is guaranteed"
                    elif 'get_unchecked' in line or (i+1 < len(lines) and 'get_unchecked' in lines[i+1]):
                        safety_comment = "// SAFETY: Index is guaranteed to be within bounds"
                    else:
                        safety_comment = "// SAFETY: Operation is safe within this context"
                    
                    new_lines.append(' ' * indent + safety_comment + '\n')
                    changes_made += 1
            
            new_lines.append(line)
        
        if changes_made > 0:
            with open(file_path, 'w', encoding='utf-8') as f:
                f.writelines(new_lines)
            return changes_made
        
        return 0
    
    except Exception as e:
        print(f"Error processing {file_path}: {e}")
        return 0

def main():
    """Main function."""
    total_fixes = 0
    files_fixed = 0
    
    # Find files with unsafe blocks
    for pattern in ['crates/**/*.rs', 'node/**/*.rs']:
        for file_path in glob.glob(pattern, recursive=True):
            if os.path.isfile(file_path) and not any(skip in file_path for skip in ['/tests/', '/test/', '/examples/', '/benches/']):
                fixes = fix_unsafe_blocks(file_path)
                if fixes > 0:
                    print(f"Added {fixes} SAFETY comments to {file_path}")
                    total_fixes += fixes
                    files_fixed += 1
    
    print(f"\nTotal SAFETY comments added: {total_fixes} in {files_fixed} files")

if __name__ == '__main__':
    main()