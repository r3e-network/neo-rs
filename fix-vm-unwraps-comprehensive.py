#!/usr/bin/env python3
"""Comprehensively fix unwraps in VM files."""

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
        
        # Fix patterns in main content only
        
        # 1. load_script().unwrap() -> load_script()?
        main_content = re.sub(
            r'engine\.load_script\(([^)]+)\)\.unwrap\(\)',
            r'engine.load_script(\1)?',
            main_content
        )
        changes_made += len(re.findall(r'engine\.load_script\([^)]+\)\.unwrap\(\)', parts[0] if parts else content))
        
        # 2. push().unwrap() -> push()?
        main_content = re.sub(
            r'\.push\(([^)]+)\)\.unwrap\(\)',
            r'.push(\1)?',
            main_content
        )
        
        # 3. pop().unwrap() -> pop()?
        main_content = re.sub(
            r'\.pop\(\)\.unwrap\(\)',
            '.pop()?',
            main_content
        )
        
        # 4. as_int().unwrap() -> as_int()?
        main_content = re.sub(
            r'\.as_int\(\)\.unwrap\(\)',
            '.as_int()?',
            main_content
        )
        
        # 5. as_bool().unwrap() -> as_bool()?
        main_content = re.sub(
            r'\.as_bool\(\)\.unwrap\(\)',
            '.as_bool()?',
            main_content
        )
        
        # 6. Double unwrap patterns
        main_content = re.sub(
            r'\.unwrap\(\)\s*\.unwrap\(\)',
            '.map_err(|e| VMError::InvalidOperation(e.to_string()))??',
            main_content
        )
        
        # 7. execute().unwrap() -> execute()?
        main_content = re.sub(
            r'\.execute\(\)\.unwrap\(\)',
            '.execute()?',
            main_content
        )
        
        # 8. get().unwrap() -> get().ok_or(VMError)?
        main_content = re.sub(
            r'\.get\(([^)]+)\)\.unwrap\(\)',
            r'.get(\1).ok_or_else(|| VMError::InvalidOperation("Index out of bounds".to_string()))?',
            main_content
        )
        
        # Reconstruct content
        content = main_content + test_content
        
        # Count changes
        changes_made = original_content.count('.unwrap()') - content.count('.unwrap()')
        
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
        if os.path.isfile(file_path) and 'test' not in file_path:
            fixes = fix_vm_unwraps(file_path)
            total_fixes += fixes
    
    print(f"\nTotal VM unwraps fixed: {total_fixes}")

if __name__ == '__main__':
    main()