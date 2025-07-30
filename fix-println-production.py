#!/usr/bin/env python3
"""Fix println! statements in production code."""

import os
import re
import glob

def fix_println_statements(file_path):
    """Replace println! with proper logging in production code."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        original_content = content
        changes_made = 0
        
        # Skip test files, examples, and CLI console code
        if any(skip in file_path for skip in ['test', '/tests/', '/examples/', 'bench', 'cli/src/console', 'cli/src/main']):
            return 0
        
        # Fix println! statements by replacing with log::info!
        pattern = r'println!\s*\('
        if re.search(pattern, content):
            # Replace println! with log::info!
            content = re.sub(
                pattern,
                'log::info!(',
                content
            )
            changes_made += len(re.findall(pattern, original_content))
        
        if content != original_content:
            with open(file_path, 'w', encoding='utf-8') as f:
                f.write(content)
            print(f"Fixed {changes_made} println! statements in {file_path}")
            return changes_made
        
        return 0
    
    except Exception as e:
        print(f"Error processing {file_path}: {e}")
        return 0

def main():
    """Main function to fix println! statements."""
    total_fixes = 0
    
    # Process all Rust files in crates and node directories
    for pattern in ['crates/**/*.rs', 'node/**/*.rs']:
        for file_path in glob.glob(pattern, recursive=True):
            if os.path.isfile(file_path):
                fixes = fix_println_statements(file_path)
                total_fixes += fixes
    
    print(f"\nTotal println! statements fixed: {total_fixes}")

if __name__ == '__main__':
    main()