#!/usr/bin/env python3
"""Fix multiple consecutive empty lines in the codebase."""

import os
import re

def fix_multiple_empty_lines():
    """Remove multiple consecutive empty lines, leaving only single empty lines."""
    files_fixed = 0
    total_fixes = 0
    
    for root, dirs, files in os.walk('.'):
        # Skip directories we don't want to process
        dirs[:] = [d for d in dirs if d not in ['.git', 'target', 'node_modules']]
        
        for file in files:
            if file.endswith('.rs'):
                filepath = os.path.join(root, file)
                try:
                    with open(filepath, 'r') as f:
                        content = f.read()
                    
                    original_content = content
                    
                    # Replace multiple consecutive empty lines with single empty line
                    # This regex matches 2 or more consecutive newlines
                    fixed_content = re.sub(r'\n\n\n+', '\n\n', content)
                    
                    if fixed_content != original_content:
                        with open(filepath, 'w') as f:
                            f.write(fixed_content)
                        
                        # Count how many fixes were made
                        fixes_in_file = len(re.findall(r'\n\n\n+', original_content))
                        total_fixes += fixes_in_file
                        files_fixed += 1
                        print(f"Fixed {fixes_in_file} multiple empty lines in: {filepath}")
                        
                except Exception as e:
                    print(f"Error processing {filepath}: {e}")
    
    print(f"\nTotal files fixed: {files_fixed}")
    print(f"Total multiple empty lines fixed: {total_fixes}")
    return files_fixed

if __name__ == '__main__':
    print("=== Fixing Multiple Empty Lines ===")
    fix_multiple_empty_lines()