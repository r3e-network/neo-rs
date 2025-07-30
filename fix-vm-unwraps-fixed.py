#!/usr/bin/env python3
"""Fix unwraps in VM files with corrected logic."""

import os
import re
import glob

def fix_vm_unwraps(file_path):
    """Fix unwraps in VM files."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        original_content = content
        changes_made = 0
        
        # Skip test sections
        if '#[cfg(test)]' in content:
            # Split content into main and test sections
            parts = content.split('#[cfg(test)]')
            main_content = parts[0]
            test_content = '#[cfg(test)]' + '#[cfg(test)]'.join(parts[1:]) if len(parts) > 1 else ''
        else:
            main_content = content
            test_content = ''
            parts = [content]  # Fix the undefined variable issue
        
        # Fix patterns in main content only
        
        # Count original unwraps in main content
        original_unwraps = main_content.count('.unwrap()')
        
        # Apply fixes
        replacements = [
            (r'engine\.load_script\(([^)]+)\)\.unwrap\(\)', r'engine.load_script(\1)?'),
            (r'\.push\(([^)]+)\)\.unwrap\(\)', r'.push(\1)?'),
            (r'\.pop\(\)\.unwrap\(\)', '.pop()?'),
            (r'\.as_int\(\)\.unwrap\(\)', '.as_int()?'),
            (r'\.as_bool\(\)\.unwrap\(\)', '.as_bool()?'),
            (r'\.execute\(\)\.unwrap\(\)', '.execute()?'),
            (r'\.peek\(([^)]+)\)\.unwrap\(\)', r'.peek(\1)?'),
            (r'\.as_integer\(\)\.unwrap\(\)', '.as_integer()?'),
            (r'\.as_bytes\(\)\.unwrap\(\)', '.as_bytes()?'),
            (r'\.to_bigint\(\)\.unwrap\(\)', '.to_bigint().ok_or_else(|| VMError::InvalidType)?'),
            (r'\.try_into\(\)\.unwrap\(\)', '.try_into().map_err(|_| VMError::InvalidType)?'),
        ]
        
        for pattern, replacement in replacements:
            main_content = re.sub(pattern, replacement, main_content)
        
        # Reconstruct content
        content = main_content + test_content
        
        # Count changes
        new_unwraps = main_content.count('.unwrap()')
        changes_made = original_unwraps - new_unwraps
        
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
    """Main function to fix VM unwraps."""
    total_fixes = 0
    
    # Process VM files
    vm_files = glob.glob('crates/vm/src/**/*.rs', recursive=True)
    
    for file_path in vm_files:
        if os.path.isfile(file_path) and 'test' not in os.path.basename(file_path):
            fixes = fix_vm_unwraps(file_path)
            total_fixes += fixes
    
    print(f"\nTotal VM unwraps fixed: {total_fixes}")
    
    # Count remaining unwraps in VM
    remaining = 0
    for file_path in vm_files:
        if os.path.isfile(file_path) and 'test' not in os.path.basename(file_path):
            with open(file_path, 'r') as f:
                content = f.read()
                # Only count in non-test sections
                if '#[cfg(test)]' in content:
                    main_content = content.split('#[cfg(test)]')[0]
                    remaining += main_content.count('.unwrap()')
                else:
                    remaining += content.count('.unwrap()')
    
    print(f"Remaining VM unwraps: {remaining}")

if __name__ == '__main__':
    main()