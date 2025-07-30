#!/usr/bin/env python3
"""Remove obvious commented code."""

import os
import re
import glob

def remove_obvious_commented_code(file_path):
    """Remove obvious commented code patterns."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        original_content = content
        
        # Skip test files
        if any(skip in file_path for skip in ['test', '/tests/', '/examples/', 'bench']):
            return 0
        
        lines = content.splitlines()
        new_lines = []
        removed_count = 0
        
        for line in lines:
            # Skip doc comments
            if line.strip().startswith('///') or line.strip().startswith('//!'):
                new_lines.append(line)
                continue
            
            # Check for obvious commented code patterns
            if line.strip().startswith('//'):
                comment_content = line.strip()[2:].strip()
                
                # Obvious code patterns
                if any([
                    re.match(r'^(let|const|mut|fn|pub|use|impl|struct|enum|trait)\s', comment_content),
                    re.match(r'^(if|else|while|for|match|return)\s', comment_content),
                    re.match(r'^\w+\s*=\s*', comment_content),  # assignments
                    re.match(r'^\w+\(', comment_content),  # function calls
                    re.match(r'^self\.', comment_content),
                    comment_content.endswith(';'),
                    comment_content in ['{', '}', '},', '};'],
                ]):
                    removed_count += 1
                    continue
            
            new_lines.append(line)
        
        if removed_count > 0:
            content = '\n'.join(new_lines)
            with open(file_path, 'w', encoding='utf-8') as f:
                f.write(content)
            return removed_count
        
        return 0
    
    except Exception as e:
        print(f"Error processing {file_path}: {e}")
        return 0

def main():
    """Main function."""
    total_removed = 0
    files_modified = 0
    
    for pattern in ['crates/**/*.rs', 'node/**/*.rs']:
        for file_path in glob.glob(pattern, recursive=True):
            if os.path.isfile(file_path):
                removed = remove_obvious_commented_code(file_path)
                if removed > 0:
                    print(f"Removed {removed} lines from {file_path}")
                    total_removed += removed
                    files_modified += 1
    
    print(f"\nTotal lines removed: {total_removed} from {files_modified} files")

if __name__ == '__main__':
    main()