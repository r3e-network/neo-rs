#!/usr/bin/env python3
"""Fix safe unwrap() calls that can be replaced with better alternatives."""

import os
import re
import glob

def fix_safe_unwraps(file_path):
    """Fix unwraps that are safe to replace."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        original_content = content
        changes_made = 0
        
        # Skip test files and example files
        if any(skip in file_path for skip in ['test', '/tests/', '/examples/', 'bench']):
            return 0
        
        # Pattern 1: Ok(/* Implementation needed */).unwrap() -> just the value
        pattern1 = r'Ok\(([^)]+)\)\.unwrap\(\)'
        if re.search(pattern1, content):
            content = re.sub(pattern1, r'\1', content)
            changes_made += len(re.findall(pattern1, original_content))
        
        # Pattern 2: Err(/* Implementation needed */).unwrap() -> panic (but we should avoid this)
        # Skip this pattern as it's intentionally failing
        
        # Pattern 3: Default::default() patterns
        pattern3 = r'([A-Za-z0-9_]+)::default\(\)\.unwrap\(\)'
        if re.search(pattern3, content):
            content = re.sub(pattern3, r'\1::default()', content)
            changes_made += len(re.findall(pattern3, original_content))
        
        # Pattern 4: new() constructors that don't fail
        pattern4 = r'([A-Za-z0-9_]+)::new\(\)\.unwrap\(\)'
        safe_new_types = ['Vec', 'HashMap', 'BTreeMap', 'HashSet', 'BTreeSet', 'String']
        for match in re.finditer(pattern4, content):
            type_name = match.group(1)
            if type_name in safe_new_types:
                content = content.replace(match.group(0), f'{type_name}::new()', 1)
                changes_made += 1
        
        # Pattern 5: .clone().unwrap() where clone cannot fail
        pattern5 = r'([a-zA-Z0-9_]+)\.clone\(\)\.unwrap\(\)'
        if re.search(pattern5, content):
            content = re.sub(pattern5, r'\1.clone()', content)
            changes_made += len(re.findall(pattern5, original_content))
        
        # Pattern 6: len/is_empty followed by access
        # Look for patterns like: if !vec.is_empty() { vec[0].unwrap() }
        # These can often use .first() or .get(0)
        
        # Pattern 7: Regex unwraps that can use expect
        pattern7 = r'Regex::new\(([^)]+)\)\.unwrap\(\)'
        if re.search(pattern7, content):
            content = re.sub(
                pattern7,
                r'Regex::new(\1).expect("Invalid regex pattern")',
                content
            )
            changes_made += len(re.findall(pattern7, original_content))
        
        # Pattern 8: Duration operations
        pattern8 = r'Duration::from_([a-z]+)\(([^)]+)\)\.unwrap\(\)'
        if re.search(pattern8, content):
            # Duration::from_* methods don't return Result, they panic on overflow
            # So .unwrap() is incorrect here
            content = re.sub(pattern8, r'Duration::from_\1(\2)', content)
            changes_made += len(re.findall(pattern8, original_content))
        
        # Pattern 9: Array/slice index access that unwraps
        pattern9 = r'\.get\((\d+)\)\.unwrap\(\)'
        for match in re.finditer(pattern9, content):
            index = match.group(1)
            # For small constant indices, we can use expect with context
            if int(index) < 10:  # Reasonable small index
                replacement = f'.get({index}).expect("Index {index} should be valid")'
                content = content.replace(match.group(0), replacement, 1)
                changes_made += 1
        
        # Pattern 10: Zero/One/Default value unwraps
        zero_patterns = [
            (r'BigInt::zero\(\)\.unwrap\(\)', 'BigInt::zero()'),
            (r'BigInt::one\(\)\.unwrap\(\)', 'BigInt::one()'),
            (r'BigUint::zero\(\)\.unwrap\(\)', 'BigUint::zero()'),
            (r'BigUint::one\(\)\.unwrap\(\)', 'BigUint::one()'),
        ]
        
        for pattern, replacement in zero_patterns:
            if re.search(pattern, content):
                content = re.sub(pattern, replacement, content)
                changes_made += len(re.findall(pattern, original_content))
        
        # Pattern 11: Thread operations
        thread_patterns = [
            (r'thread::spawn\(([^)]+)\)\.unwrap\(\)', r'thread::spawn(\1).expect("Failed to spawn thread")'),
            (r'\.join\(\)\.unwrap\(\)', '.join().expect("Thread panicked")'),
        ]
        
        for pattern, replacement in thread_patterns:
            if re.search(pattern, content):
                content = re.sub(pattern, replacement, content)
                changes_made += len(re.findall(pattern, original_content))
        
        # Pattern 12: Environment variables
        pattern12 = r'env::var\(([^)]+)\)\.unwrap\(\)'
        if re.search(pattern12, content):
            content = re.sub(
                pattern12,
                r'env::var(\1).expect("Environment variable not set")',
                content
            )
            changes_made += len(re.findall(pattern12, original_content))
        
        if content != original_content:
            with open(file_path, 'w', encoding='utf-8') as f:
                f.write(content)
            print(f"Fixed {changes_made} safe unwraps in {file_path}")
            return changes_made
        
        return 0
    
    except Exception as e:
        print(f"Error processing {file_path}: {e}")
        return 0

def main():
    """Main function to fix safe unwraps."""
    total_fixes = 0
    
    # Process all Rust files in crates and node directories
    for pattern in ['crates/**/*.rs', 'node/**/*.rs']:
        for file_path in glob.glob(pattern, recursive=True):
            if os.path.isfile(file_path):
                fixes = fix_safe_unwraps(file_path)
                total_fixes += fixes
    
    print(f"\nTotal safe unwraps fixed: {total_fixes}")

if __name__ == '__main__':
    main()