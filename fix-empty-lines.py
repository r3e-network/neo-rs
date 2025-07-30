#!/usr/bin/env python3
"""Fix multiple consecutive empty lines."""

import os
import re
import glob

def fix_empty_lines(file_path):
    """Fix multiple consecutive empty lines in a file."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        # Replace multiple consecutive empty lines with single empty line
        original_content = content
        content = re.sub(r'\n\n\n+', '\n\n', content)
        
        if content != original_content:
            with open(file_path, 'w', encoding='utf-8') as f:
                f.write(content)
            # Count how many instances were fixed
            fixes = len(re.findall(r'\n\n\n+', original_content))
            return fixes
        
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
            if os.path.isfile(file_path) and not any(skip in file_path for skip in ['/tests/', '/test/', '/examples/', '/benches/']):
                fixes = fix_empty_lines(file_path)
                if fixes > 0:
                    print(f"Fixed {fixes} multiple empty line instances in {file_path}")
                    total_fixes += fixes
                    files_fixed += 1
    
    print(f"\nTotal multiple empty lines fixed: {total_fixes} in {files_fixed} files")

if __name__ == '__main__':
    main()