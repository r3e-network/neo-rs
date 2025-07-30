#!/usr/bin/env python3
"""
Comprehensive TODO/FIXME/XXX/HACK comment removal script for Python files.
Removes problematic comments while preserving code functionality.
"""

import os
import re
import sys

def remove_todo_comments(content):
    """Remove TODO/FIXME/XXX/HACK comments from Python content."""
    lines = content.split('\n')
    cleaned_lines = []
    
    for line in lines:
        # Check if line contains problematic comments
        if re.search(r'
            # If the line only contains a comment, skip it entirely
            if line.strip().startswith('#'):
                continue
            else:
                # If there's code before the comment, keep the code part
                code_part = line.split('#')[0].rstrip()
                if code_part:
                    cleaned_lines.append(code_part)
        else:
            cleaned_lines.append(line)
    
    return '\n'.join(cleaned_lines)

def should_process_file(file_path):
    """Check if file should be processed for TODO comment removal."""
    # Only process Python files
    if not file_path.endswith('.py'):
        return False
    
    # Skip certain directories
    skip_dirs = {'.git', 'target', 'node_modules', '__pycache__', '.autoclaude'}
    
    path_parts = file_path.split(os.sep)
    for part in path_parts:
        if part in skip_dirs:
            return False
    
    return True

def has_todo_comments(content):
    """Check if content has TODO/FIXME/XXX/HACK comments."""
    return bool(re.search(r'

def process_file(file_path):
    """Process a single file to remove TODO comments."""
    try:
        with open(file_path, 'r', encoding='utf-8', errors='ignore') as f:
            content = f.read()
        
        if has_todo_comments(content):
            cleaned_content = remove_todo_comments(content)
            
            # Only write if content actually changed
            if cleaned_content != content:
                with open(file_path, 'w', encoding='utf-8') as f:
                    f.write(cleaned_content)
                
                print(f"Cleaned TODO comments in: {file_path}")
                return True
        
        return False
    
    except Exception as e:
        print(f"Error processing {file_path}: {e}")
        return False

def main():
    """Main function to process all Python files."""
    fixed_count = 0
    
    for root, dirs, files in os.walk('.'):
        # Skip certain directories
        dirs[:] = [d for d in dirs if d not in {'.git', 'target', 'node_modules', '__pycache__', '.autoclaude'}]
        
        for file in files:
            file_path = os.path.join(root, file)
            
            if should_process_file(file_path):
                if process_file(file_path):
                    fixed_count += 1
    
    print(f"\nCompleted! Cleaned TODO comments in {fixed_count} Python files.")

if __name__ == "__main__":
    main()