#!/usr/bin/env python3
"""Remove all merge conflict markers from source files."""

import os
import re

def remove_merge_conflicts(content):
    """Remove merge conflict markers by keeping the current version."""
    lines = content.split('\n')
    result_lines = []
    in_conflict = False
    keep_current = True
    
    i = 0
    while i < len(lines):
        line = lines[i]
        
        if line.startswith('<<<<<<< '):
            in_conflict = True
            keep_current = True
            i += 1
            continue
        elif line.strip() == '=======':
            if in_conflict:
                keep_current = False
            i += 1
            continue
        elif line.startswith('>>>>>>> '):
            in_conflict = False
            keep_current = True
            i += 1
            continue
        
        # Keep lines when not in conflict or when keeping current branch
        if not in_conflict or keep_current:
            result_lines.append(line)
        
        i += 1
    
    return '\n'.join(result_lines)

def process_file(filepath):
    """Process a single file to remove merge conflict markers."""
    try:
        with open(filepath, 'r', encoding='utf-8') as f:
            content = f.read()
        
        # Check if file has merge conflicts
        if '<<<<<<< ' in content or '=======' in content or '>>>>>>> ' in content:
            fixed_content = remove_merge_conflicts(content)
            with open(filepath, 'w', encoding='utf-8') as f:
                f.write(fixed_content)
            print(f"Fixed merge conflicts in: {filepath}")
            return True
        return False
    except Exception as e:
        print(f"Error processing {filepath}: {e}")
        return False

def main():
    """Main function to process all source files."""
    extensions = ['.rs', '.py', '.js', '.md', '.toml', '.yml', '.yaml', '.json', '.cs']
    directories = ['crates', 'node', 'tests', 'demo', 'dev', 'scripts', 'neo_csharp', '.']
    
    total_fixed = 0
    
    for directory in directories:
        if not os.path.exists(directory):
            continue
            
        for root, _, files in os.walk(directory):
            # Skip hidden directories and build directories
            if any(skip in root for skip in ['/.git/', '/target/', '/node_modules/', '/.autoclaude/']):
                continue
                
            for filename in files:
                # Check if file has a relevant extension
                if any(filename.endswith(ext) for ext in extensions):
                    filepath = os.path.join(root, filename)
                    if process_file(filepath):
                        total_fixed += 1
    
    print(f"\nTotal files fixed: {total_fixed}")

if __name__ == "__main__":
    main()