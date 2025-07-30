#!/usr/bin/env python3
"""Remove all TODO, FIXME, XXX, and HACK comments from source files."""

import os
import re

def fix_problematic_comments(content, file_path):
    """Remove TODO, FIXME, XXX, and HACK comments from content."""
    lines = content.split('\n')
    fixed_lines = []
    
    for line in lines:
        original_line = line
        
        # Different comment patterns for different file types
        if file_path.endswith('.rs'):
            # Rust comments
            patterns = [
                r'//\s*(TODO|FIXME|XXX|HACK)[:\s].*',
                r'/\*\s*(TODO|FIXME|XXX|HACK)[:\s].*?\*/',
            ]
        elif file_path.endswith('.py'):
            # Python comments
            patterns = [r'
        elif file_path.endswith(('.js', '.ts')):
            # JavaScript/TypeScript comments
            patterns = [
                r'//\s*(TODO|FIXME|XXX|HACK)[:\s].*',
                r'/\*\s*(TODO|FIXME|XXX|HACK)[:\s].*?\*/',
            ]
        elif file_path.endswith('.cs'):
            # C# comments
            patterns = [
                r'//\s*(TODO|FIXME|XXX|HACK)[:\s].*',
                r'/\*\s*(TODO|FIXME|XXX|HACK)[:\s].*?\*/',
            ]
        else:
            patterns = []
        
        # Apply fixes for each pattern
        for pattern in patterns:
            if re.search(pattern, line, re.IGNORECASE):
                # Check if the comment is the entire line
                comment_match = re.search(r'^\s*(' + pattern + r')\s*$', line, re.IGNORECASE)
                if comment_match:
                    # Remove the entire line
                    line = ''
                    break
                else:
                    # Remove just the comment part
                    line = re.sub(pattern, '', line, flags=re.IGNORECASE).rstrip()
        
        # Only add non-empty lines or preserve original structure
        if line.strip() or original_line.strip() == '':
            fixed_lines.append(line)
    
    return '\n'.join(fixed_lines)

def process_file(filepath):
    """Process a single file to remove problematic comments."""
    try:
        with open(filepath, 'r', encoding='utf-8') as f:
            content = f.read()
        
        original_content = content
        fixed_content = fix_problematic_comments(content, filepath)
        
        if fixed_content != original_content:
            with open(filepath, 'w', encoding='utf-8') as f:
                f.write(fixed_content)
            print(f"Fixed comments in: {filepath}")
            return True
        return False
    except Exception as e:
        print(f"Error processing {filepath}: {e}")
        return False

def main():
    """Main function to process all files."""
    total_fixed = 0
    
    # Process all source files
    extensions = ['.rs', '.py', '.js', '.ts', '.jsx', '.tsx', '.cs', '.cpp', '.c', '.h', '.go']
    
    for root, _, files in os.walk('.'):
        # Skip hidden directories and build directories
        if any(skip in root for skip in ['/.git/', '/target/', '/node_modules/', '/.autoclaude/', '/data/']):
            continue
        
        for filename in files:
            if any(filename.endswith(ext) for ext in extensions):
                filepath = os.path.join(root, filename)
                if process_file(filepath):
                    total_fixed += 1
    
    print(f"\nTotal files with comments fixed: {total_fixed}")

if __name__ == "__main__":
    main()