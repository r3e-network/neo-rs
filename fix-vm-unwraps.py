#!/usr/bin/env python3
"""Fix unwrap() calls in VM module with proper error handling."""

import os
import re
import glob

def fix_vm_unwraps(file_path):
    """Fix unwraps in VM module files."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        original_content = content
        changes_made = 0
        
        # Skip test files
        if 'test' in file_path or '/tests/' in file_path:
            return 0
        
        # Fix engine.current_context_mut().unwrap() patterns
        pattern1 = r'engine\.current_context_mut\(\)\.unwrap\(\)'
        if re.search(pattern1, content):
            content = re.sub(
                pattern1, 
                'engine.current_context_mut().ok_or_else(|| VMError::InvalidOperation("No current context".to_string()))?',
                content
            )
            changes_made += len(re.findall(pattern1, original_content))
        
        # Fix engine.current_context().unwrap() patterns
        pattern2 = r'engine\.current_context\(\)\.unwrap\(\)'
        if re.search(pattern2, content):
            content = re.sub(
                pattern2,
                'engine.current_context().ok_or_else(|| VMError::InvalidOperation("No current context".to_string()))?',
                content
            )
            changes_made += len(re.findall(pattern2, original_content))
        
        # Fix stack.pop().unwrap() patterns
        pattern3 = r'stack\.pop\(\)\.unwrap\(\)'
        if re.search(pattern3, content):
            content = re.sub(
                pattern3,
                'stack.pop().ok_or_else(|| VMError::StackUnderflow)?',
                content
            )
            changes_made += len(re.findall(pattern3, original_content))
        
        # Fix .try_into().unwrap() patterns
        pattern4 = r'\.try_into\(\)\.unwrap\(\)'
        if re.search(pattern4, content):
            content = re.sub(
                pattern4,
                '.try_into().map_err(|_| VMError::InvalidType)?',
                content
            )
            changes_made += len(re.findall(pattern4, original_content))
        
        # Fix .to_integer().unwrap() patterns
        pattern5 = r'\.to_integer\(\)\.unwrap\(\)'
        if re.search(pattern5, content):
            content = re.sub(
                pattern5,
                '.to_integer().ok_or_else(|| VMError::InvalidType)?',
                content
            )
            changes_made += len(re.findall(pattern5, original_content))
        
        # Fix .as_bool().unwrap() patterns
        pattern6 = r'\.as_bool\(\)\.unwrap\(\)'
        if re.search(pattern6, content):
            content = re.sub(
                pattern6,
                '.as_bool().ok_or_else(|| VMError::InvalidType)?',
                content
            )
            changes_made += len(re.findall(pattern6, original_content))
        
        # Add VMError import if needed and changes were made
        if changes_made > 0 and 'use crate::error::VMError' not in content:
            # Find the last use statement
            use_pattern = r'(use [^;]+;\n)'
            use_matches = list(re.finditer(use_pattern, content))
            if use_matches:
                last_use = use_matches[-1]
                insert_pos = last_use.end()
                content = content[:insert_pos] + 'use crate::error::VMError;\n' + content[insert_pos:]
        
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
    
    # Process VM module files
    for file_path in glob.glob('crates/vm/src/**/*.rs', recursive=True):
        if os.path.isfile(file_path):
            fixes = fix_vm_unwraps(file_path)
            total_fixes += fixes
    
    print(f"\nTotal VM unwraps fixed: {total_fixes}")

if __name__ == '__main__':
    main()