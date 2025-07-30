#!/usr/bin/env python3
"""Fix common unwrap() patterns in production code."""

import os
import re
import glob

def fix_unwraps_in_file(file_path):
    """Fix unwraps in a single file."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        original_content = content
        changes_made = 0
        
        # Skip test files
        if 'test' in file_path or '/tests/' in file_path:
            return 0
        
        # Fix SystemTime::now().duration_since(UNIX_EPOCH).unwrap()
        pattern1 = r'SystemTime::now\(\)\s*\.duration_since\(UNIX_EPOCH\)\s*\.unwrap\(\)'
        if re.search(pattern1, content):
            content = re.sub(pattern1, 'SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default()', content)
            changes_made += len(re.findall(pattern1, original_content))
        
        # Fix .parse().unwrap() for known safe operations
        pattern2 = r'\.parse\(\)\s*\.unwrap\(\)'
        matches = re.findall(r'(\w+)\.parse\(\)\.unwrap\(\)', content)
        for match in matches:
            old_pattern = f'{match}.parse().unwrap()'
            new_pattern = f'{match}.parse().expect("Failed to parse {match}")'
            content = content.replace(old_pattern, new_pattern)
            changes_made += 1
        
        # Fix .to_string().parse().unwrap() patterns
        pattern3 = r'\.to_string\(\)\s*\.parse\(\)\s*\.unwrap\(\)'
        if re.search(pattern3, content):
            content = re.sub(pattern3, '.to_string().parse().expect("Failed to parse string")', content)
            changes_made += len(re.findall(pattern3, original_content))
        
        # Fix .lock().unwrap() with better error message
        pattern4 = r'\.lock\(\)\s*\.unwrap\(\)'
        if re.search(pattern4, content):
            content = re.sub(pattern4, '.lock().expect("Failed to acquire lock")', content)
            changes_made += len(re.findall(pattern4, original_content))
        
        # Fix .join().unwrap() for thread operations
        pattern5 = r'\.join\(\)\s*\.unwrap\(\)'
        if re.search(pattern5, content):
            content = re.sub(pattern5, '.join().expect("Failed to join thread")', content)
            changes_made += len(re.findall(pattern5, original_content))
        
        # Fix .recv().unwrap() for channel operations
        pattern6 = r'\.recv\(\)\s*\.unwrap\(\)'
        if re.search(pattern6, content):
            content = re.sub(pattern6, '.recv().expect("Failed to receive from channel")', content)
            changes_made += len(re.findall(pattern6, original_content))
        
        # Fix .send().unwrap() for channel operations
        pattern7 = r'\.send\([^)]+\)\s*\.unwrap\(\)'
        if re.search(pattern7, content):
            content = re.sub(pattern7, lambda m: m.group(0).replace('.unwrap()', '.expect("Failed to send to channel")'), content)
            changes_made += len(re.findall(pattern7, original_content))
        
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
    """Main function to fix unwraps in all Rust files."""
    total_fixes = 0
    
    # Process all Rust files in crates and node directories
    for pattern in ['crates/**/*.rs', 'node/**/*.rs']:
        for file_path in glob.glob(pattern, recursive=True):
            if os.path.isfile(file_path):
                fixes = fix_unwraps_in_file(file_path)
                total_fixes += fixes
    
    print(f"\nTotal unwraps fixed: {total_fixes}")

if __name__ == '__main__':
    main()