#!/usr/bin/env python3
"""Fix critical unwraps in production code v2."""

import os
import re
import glob

def fix_critical_unwraps(file_path):
    """Fix critical unwrap patterns."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        # Skip test files
        if any(skip in file_path for skip in ['test', '/tests/', '/examples/', 'bench']):
            return 0
        
        original_content = content
        changes_made = 0
        
        # Simple replacements
        replacements = [
            # Arc::new(Mutex::new(/* Implementation needed */)).unwrap() doesn't make sense - remove .unwrap()
            (r'Arc::new\(Mutex::new\([^)]+\)\)\.unwrap\(\)', lambda m: m.group(0).replace('.unwrap()', '')),
            
            # .clone().unwrap() -> .clone()
            (r'\.clone\(\)\.unwrap\(\)', '.clone()'),
            
            # Default::default().unwrap() -> Default::default()
            (r'Default::default\(\)\.unwrap\(\)', 'Default::default()'),
            
            # Vec::new().unwrap() -> Vec::new()
            (r'Vec::new\(\)\.unwrap\(\)', 'Vec::new()'),
            
            # HashMap::new().unwrap() -> HashMap::new()
            (r'HashMap::new\(\)\.unwrap\(\)', 'HashMap::new()'),
        ]
        
        for pattern, replacement in replacements:
            if isinstance(replacement, str):
                content = re.sub(pattern, replacement, content)
            else:
                content = re.sub(pattern, replacement, content)
        
        # Count changes
        changes_made = content.count('unwrap()') - original_content.count('unwrap()')
        changes_made = abs(changes_made)
        
        if content != original_content:
            with open(file_path, 'w', encoding='utf-8') as f:
                f.write(content)
            return changes_made
        
        return 0
    
    except Exception as e:
        print(f"Error processing {file_path}: {e}")
        return 0

def main():
    """Main function."""
    total_fixes = 0
    files_fixed = 0
    
    for pattern in ['crates/**/*.rs', 'node/**/*.rs']:
        for file_path in glob.glob(pattern, recursive=True):
            if os.path.isfile(file_path):
                fixes = fix_critical_unwraps(file_path)
                if fixes > 0:
                    print(f"Fixed {fixes} unwrap() calls in {file_path}")
                    total_fixes += fixes
                    files_fixed += 1
    
    print(f"\nTotal files processed with unwrap fixes: {files_fixed}")

if __name__ == '__main__':
    main()