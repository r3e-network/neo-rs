#!/usr/bin/env python3
"""
Comprehensive merge conflict marker removal script.
Removes all Git merge conflict markers from source files.
"""

import os
import re
import sys

def remove_merge_conflicts(content):
    """Remove merge conflict markers by keeping the current version."""
    lines = content.split('\n')
    result_lines = []
    in_conflict = False
    skip_until_end = False
    
    for line in lines:
        # Check for start of conflict
        if line.startswith('<<<<<<<'):
            in_conflict = True
            continue
        
        # Check for middle separator
        if line.startswith('======='):
            skip_until_end = True
            continue
        
        # Check for end of conflict
        if line.startswith('>>>>>>>'):
            in_conflict = False
            skip_until_end = False
            continue
        
        # Include line if not in conflict zone or if before separator
        if not in_conflict or not skip_until_end:
            result_lines.append(line)
    
    return '\n'.join(result_lines)

def should_process_file(file_path):
    """Check if file should be processed for merge conflict removal."""
    # Skip certain file types and directories
    skip_extensions = {'.png', '.jpg', '.jpeg', '.gif', '.ico', '.pdf', '.zip', '.tar', '.gz'}
    skip_dirs = {'.git', 'target', 'node_modules', '__pycache__', '.autoclaude'}
    
    # Check extension
    _, ext = os.path.splitext(file_path)
    if ext.lower() in skip_extensions:
        return False
    
    # Check directory
    path_parts = file_path.split(os.sep)
    for part in path_parts:
        if part in skip_dirs:
            return False
    
    return True

def has_merge_conflicts(content):
    """Check if content has merge conflict markers."""
    return ('<<<<<<< ' in content or 
            '=======' in content or 
            '>>>>>>> ' in content)

def process_file(file_path):
    """Process a single file to remove merge conflicts."""
    try:
        with open(file_path, 'r', encoding='utf-8', errors='ignore') as f:
            content = f.read()
        
        if has_merge_conflicts(content):
            cleaned_content = remove_merge_conflicts(content)
            
            with open(file_path, 'w', encoding='utf-8') as f:
                f.write(cleaned_content)
            
            print(f"Fixed merge conflicts in: {file_path}")
            return True
        
        return False
    
    except Exception as e:
        print(f"Error processing {file_path}: {e}")
        return False

def main():
    """Main function to process all files."""
    fixed_count = 0
    
    for root, dirs, files in os.walk('.'):
        # Skip certain directories
        dirs[:] = [d for d in dirs if d not in {'.git', 'target', 'node_modules', '__pycache__', '.autoclaude'}]
        
        for file in files:
            file_path = os.path.join(root, file)
            
            if should_process_file(file_path):
                if process_file(file_path):
                    fixed_count += 1
    
    print(f"\nCompleted! Fixed merge conflicts in {fixed_count} files.")

if __name__ == "__main__":
    main()