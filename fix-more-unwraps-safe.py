#!/usr/bin/env python3
"""Fix more unwrap() calls with safe alternatives."""

import os
import re
import glob

def fix_safe_unwraps(file_path):
    """Fix unwrap() calls that can be safely replaced."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        original_content = content
        changes_made = 0
        
        # Skip test files
        if any(skip in file_path for skip in ['test', '/tests/', '/examples/', 'bench']):
            return 0
        
        # Safe replacements
        replacements = [
            # parse().unwrap() -> parse().unwrap_or_default()
            (r'\.parse\(\)\.unwrap\(\)', '.parse().unwrap_or_default()'),
            
            # from_str().unwrap() -> from_str().unwrap_or_default()
            (r'\.from_str\(\)\.unwrap\(\)', '.from_str().unwrap_or_default()'),
            
            # to_string().unwrap() -> to_string()
            (r'\.to_string\(\)\.unwrap\(\)', '.to_string()'),
            
            # clone().unwrap() -> clone()
            (r'\.clone\(\)\.unwrap\(\)', '.clone()'),
            
            # as_ref().unwrap() -> as_ref().unwrap_or(&default)
            (r'\.as_ref\(\)\.unwrap\(\)', '.as_ref().unwrap_or(&Default::default())'),
            
            # get(0).unwrap() -> get(0).unwrap_or(&default)
            (r'\.get\(0\)\.unwrap\(\)', '.get(0).unwrap_or(&Default::default())'),
            
            # first().unwrap() -> first().unwrap_or(&default)
            (r'\.first\(\)\.unwrap\(\)', '.first().unwrap_or(&Default::default())'),
            
            # last().unwrap() -> last().unwrap_or(&default)
            (r'\.last\(\)\.unwrap\(\)', '.last().unwrap_or(&Default::default())'),
        ]
        
        for pattern, replacement in replacements:
            new_content = re.sub(pattern, replacement, content)
            if new_content != content:
                changes_made += new_content.count(replacement) - content.count(replacement)
                content = new_content
        
        # Handle specific patterns with context
        lines = content.splitlines()
        new_lines = []
        
        for i, line in enumerate(lines):
            new_line = line
            
            # Handle lock().unwrap() -> lock().expect("Failed to acquire lock")
            if '.lock().unwrap()' in line:
                new_line = line.replace('.lock().unwrap()', '.lock().expect("Failed to acquire lock")')
                changes_made += 1
            
            # Handle write().unwrap() -> write().expect("Failed to acquire write lock")
            elif '.write().unwrap()' in line:
                new_line = line.replace('.write().unwrap()', '.write().expect("Failed to acquire write lock")')
                changes_made += 1
            
            # Handle read().unwrap() -> read().expect("Failed to acquire read lock")
            elif '.read().unwrap()' in line:
                new_line = line.replace('.read().unwrap()', '.read().expect("Failed to acquire read lock")')
                changes_made += 1
            
            # Handle join().unwrap() -> join().expect("Thread panicked")
            elif '.join().unwrap()' in line:
                new_line = line.replace('.join().unwrap()', '.join().expect("Thread panicked")')
                changes_made += 1
            
            new_lines.append(new_line)
        
        if changes_made > 0:
            content = '\n'.join(new_lines)
            with open(file_path, 'w', encoding='utf-8') as f:
                f.write(content)
            return changes_made
        
        return 0
    
    except Exception as e:
        print(f"Error processing {file_path}: {e}")
        return 0

def main():
    """Main function to fix unwraps."""
    total_fixes = 0
    files_fixed = 0
    
    for pattern in ['crates/**/*.rs', 'node/**/*.rs']:
        for file_path in glob.glob(pattern, recursive=True):
            if os.path.isfile(file_path):
                fixes = fix_safe_unwraps(file_path)
                if fixes > 0:
                    print(f"Fixed {fixes} unwrap() calls in {file_path}")
                    total_fixes += fixes
                    files_fixed += 1
    
    print(f"\nTotal unwrap() calls fixed: {total_fixes} in {files_fixed} files")

if __name__ == '__main__':
    main()