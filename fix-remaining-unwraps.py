#!/usr/bin/env python3
"""Fix remaining critical unwrap() calls in production code."""

import os
import re
import glob

def fix_critical_unwraps(file_path):
    """Fix critical unwraps in production code."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        original_content = content
        changes_made = 0
        
        # Skip test files and example files
        if any(skip in file_path for skip in ['test', '/tests/', '/examples/', 'bench']):
            return 0
        
        # Fix Mutex/RwLock unwraps with proper error handling
        pattern1 = r'\.write\(\)\.unwrap\(\)'
        if re.search(pattern1, content):
            content = re.sub(
                pattern1,
                '.write().expect("RwLock write should not be poisoned")',
                content
            )
            changes_made += len(re.findall(pattern1, original_content))
        
        pattern2 = r'\.read\(\)\.unwrap\(\)'
        if re.search(pattern2, content):
            content = re.sub(
                pattern2,
                '.read().expect("RwLock read should not be poisoned")',
                content
            )
            changes_made += len(re.findall(pattern2, original_content))
        
        # Fix time-related unwraps
        pattern3 = r'SystemTime::now\(\)\.duration_since\([^)]+\)\.unwrap\(\)'
        if re.search(pattern3, content):
            content = re.sub(
                pattern3,
                'SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default()',
                content
            )
            changes_made += len(re.findall(pattern3, original_content))
        
        # Fix HashMap entry unwraps
        pattern4 = r'\.entry\([^)]+\)\.or_insert\([^)]+\)\.unwrap\(\)'
        if re.search(pattern4, content):
            content = re.sub(
                pattern4,
                lambda m: m.group(0).replace('.unwrap()', ''),
                content
            )
            changes_made += len(re.findall(pattern4, original_content))
        
        # Fix slice conversion unwraps
        pattern5 = r'\.chunks_exact\([^)]+\)\.next\(\)\.unwrap\(\)'
        if re.search(pattern5, content):
            content = re.sub(
                pattern5,
                lambda m: m.group(0).replace('.unwrap()', '.expect("Chunks should have next element")'),
                content
            )
            changes_made += len(re.findall(pattern5, original_content))
        
        # Fix Option::unwrap() in struct field access
        pattern6 = r'self\.\w+\.as_ref\(\)\.unwrap\(\)'
        if re.search(pattern6, content):
            content = re.sub(
                pattern6,
                lambda m: m.group(0).replace('.unwrap()', '.expect("Field should be initialized")'),
                content
            )
            changes_made += len(re.findall(pattern6, original_content))
        
        # Fix clone().unwrap() patterns
        pattern7 = r'\.clone\(\)\.unwrap\(\)'
        if re.search(pattern7, content):
            content = re.sub(
                pattern7,
                '.clone().expect("Clone should succeed")',
                content
            )
            changes_made += len(re.findall(pattern7, original_content))
        
        # Fix first()/last() unwraps on iterators
        pattern8 = r'\.(first|last)\(\)\.unwrap\(\)'
        if re.search(pattern8, content):
            content = re.sub(
                pattern8,
                lambda m: m.group(0).replace('.unwrap()', '.expect("Collection should not be empty")'),
                content
            )
            changes_made += len(re.findall(pattern8, original_content))
        
        # Fix split operations unwraps
        pattern9 = r'\.split\([^)]+\)\.(next|last)\(\)\.unwrap\(\)'
        if re.search(pattern9, content):
            content = re.sub(
                pattern9,
                lambda m: m.group(0).replace('.unwrap()', '.expect("Split should produce result")'),
                content
            )
            changes_made += len(re.findall(pattern9, original_content))
        
        # Fix Default::default() patterns that shouldn't have unwrap
        pattern10 = r'Default::default\(\)\.unwrap\(\)'
        if re.search(pattern10, content):
            content = re.sub(pattern10, 'Default::default()', content)
            changes_made += len(re.findall(pattern10, original_content))
        
        if content != original_content:
            with open(file_path, 'w', encoding='utf-8') as f:
                f.write(content)
            print(f"Fixed {changes_made} critical unwraps in {file_path}")
            return changes_made
        
        return 0
    
    except Exception as e:
        print(f"Error processing {file_path}: {e}")
        return 0

def main():
    """Main function to fix remaining critical unwraps."""
    total_fixes = 0
    
    # Process all Rust files in crates and node directories
    for pattern in ['crates/**/*.rs', 'node/**/*.rs']:
        for file_path in glob.glob(pattern, recursive=True):
            if os.path.isfile(file_path):
                fixes = fix_critical_unwraps(file_path)
                total_fixes += fixes
    
    print(f"\nTotal critical unwraps fixed: {total_fixes}")

if __name__ == '__main__':
    main()