#!/usr/bin/env python3
"""Fix unwraps in VM bitwise operations."""

import os
import re

def fix_vm_bitwise_unwraps():
    """Fix unwraps in the VM bitwise operations file."""
    file_path = 'crates/vm/src/jump_table/bitwise.rs'
    
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        original_content = content
        changes_made = 0
        
        # Common patterns in VM bitwise operations
        
        # 1. pop().unwrap() -> pop()?
        content = re.sub(
            r'engine\.evaluation_stack\.pop\(\)\.unwrap\(\)',
            'engine.evaluation_stack.pop()?',
            content
        )
        changes_made += len(re.findall(r'engine\.evaluation_stack\.pop\(\)\.unwrap\(\)', original_content))
        
        # 2. as_integer().unwrap() -> as_integer()?
        content = re.sub(
            r'\.as_integer\(\)\.unwrap\(\)',
            '.as_integer()?',
            content
        )
        changes_made += len(re.findall(r'\.as_integer\(\)\.unwrap\(\)', original_content))
        
        # 3. as_bytes().unwrap() -> as_bytes()?
        content = re.sub(
            r'\.as_bytes\(\)\.unwrap\(\)',
            '.as_bytes()?',
            content
        )
        changes_made += len(re.findall(r'\.as_bytes\(\)\.unwrap\(\)', original_content))
        
        # 4. to_bigint().unwrap() -> to_bigint().ok_or_else(|| VMError::InvalidType)?
        content = re.sub(
            r'\.to_bigint\(\)\.unwrap\(\)',
            '.to_bigint().ok_or_else(|| VMError::InvalidType)?',
            content
        )
        changes_made += len(re.findall(r'\.to_bigint\(\)\.unwrap\(\)', original_content))
        
        # 5. Fix peek().unwrap() patterns
        content = re.sub(
            r'engine\.evaluation_stack\.peek\(([^)]+)\)\.unwrap\(\)',
            r'engine.evaluation_stack.peek(\1)?',
            content
        )
        changes_made += len(re.findall(r'engine\.evaluation_stack\.peek\([^)]+\)\.unwrap\(\)', original_content))
        
        # 6. Fix try_into().unwrap() for conversions
        content = re.sub(
            r'\.try_into\(\)\.unwrap\(\)',
            '.try_into().map_err(|_| VMError::InvalidType)?',
            content
        )
        changes_made += len(re.findall(r'\.try_into\(\)\.unwrap\(\)', original_content))
        
        # 7. Fix array index access
        content = re.sub(
            r'(\w+)\[(\d+)\]\.clone\(\)\.unwrap\(\)',
            r'\1.get(\2).ok_or_else(|| VMError::InvalidOperation)?.clone()',
            content
        )
        
        # Write back if changes were made
        if content != original_content:
            with open(file_path, 'w', encoding='utf-8') as f:
                f.write(content)
            print(f"Fixed {changes_made} unwraps in {file_path}")
            
            # Count remaining unwraps
            remaining = len(re.findall(r'\.unwrap\(\)', content))
            print(f"Remaining unwraps in file: {remaining}")
        
        return changes_made
    
    except Exception as e:
        print(f"Error processing {file_path}: {e}")
        return 0

if __name__ == '__main__':
    fix_vm_bitwise_unwraps()