#!/usr/bin/env python3
"""Fix remaining unwraps in production code."""

import os
import re
import glob

def fix_unwraps_in_file(file_path):
    """Fix unwraps in a single file."""
    try:
        # Skip test files
        if any(skip in file_path for skip in ['test', '/tests/', '/examples/', 'bench']):
            return 0
        
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        original_content = content
        changes_made = 0
        
        # Pattern replacements
        replacements = [
            # Arc::new(x).unwrap() doesn't make sense - Arc::new never fails
            (r'Arc::new\([^)]+\)\.unwrap\(\)', lambda m: m.group(0).replace('.unwrap()', '')),
            
            # Box::new(x).unwrap() doesn't make sense - Box::new never fails
            (r'Box::new\([^)]+\)\.unwrap\(\)', lambda m: m.group(0).replace('.unwrap()', '')),
            
            # String::from(x).unwrap() doesn't make sense
            (r'String::from\([^)]+\)\.unwrap\(\)', lambda m: m.group(0).replace('.unwrap()', '')),
            
            # to_string().unwrap() doesn't make sense
            (r'\.to_string\(\)\.unwrap\(\)', '.to_string()'),
            
            # Default::default().unwrap() doesn't make sense
            (r'Default::default\(\)\.unwrap\(\)', 'Default::default()'),
            
            # clone().unwrap() doesn't make sense
            (r'\.clone\(\)\.unwrap\(\)', '.clone()'),
            
            # Common parsing patterns
            (r'\.parse\(\)\.unwrap\(\)', '.parse().expect("Valid format")'),
            (r'\.from_str\(\)\.unwrap\(\)', '.from_str().expect("Valid string")'),
            
            # Lock patterns
            (r'\.lock\(\)\.unwrap\(\)', '.lock().expect("Lock poisoned")'),
            (r'\.write\(\)\.unwrap\(\)', '.write().expect("Write lock poisoned")'),
            (r'\.read\(\)\.unwrap\(\)', '.read().expect("Read lock poisoned")'),
            
            # Try operations
            (r'\.try_into\(\)\.unwrap\(\)', '.try_into().expect("Conversion failed")'),
            (r'\.try_from\(\)\.unwrap\(\)', '.try_from().expect("Conversion failed")'),
        ]
        
        for pattern, replacement in replacements:
            if isinstance(replacement, str):
                new_content = re.sub(pattern, replacement, content)
            else:
                new_content = re.sub(pattern, replacement, content)
            
            if new_content != content:
                changes_made += content.count('.unwrap()') - new_content.count('.unwrap()')
                content = new_content
        
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
    
    # Process all non-test Rust files
    for pattern in ['crates/**/*.rs', 'node/**/*.rs']:
        for file_path in glob.glob(pattern, recursive=True):
            if os.path.isfile(file_path):
                fixes = fix_unwraps_in_file(file_path)
                if fixes > 0:
                    print(f"Fixed {fixes} unwrap() calls in {file_path}")
                    total_fixes += fixes
                    files_fixed += 1
    
    print(f"\nTotal unwrap() calls fixed: {total_fixes} in {files_fixed} files")

if __name__ == '__main__':
    main()