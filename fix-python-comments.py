#!/usr/bin/env python3
"""Remove TODO, FIXME, XXX, and HACK comments from Python scripts."""

import os
import re

def remove_problematic_comments(content):
    """Remove TODO, FIXME, XXX, and HACK comments from content."""
    lines = content.split('\n')
    fixed_lines = []
    
    for line in lines:
        # Pattern to match these comments
        pattern = r'
        
        # Check if line contains problematic comment
        match = re.search(pattern, line, re.IGNORECASE)
        if match:
            # Remove the comment part
            comment_start = line.find('#')
            if comment_start >= 0:
                # Keep the code part before the comment
                code_part = line[:comment_start].rstrip()
                if code_part:
                    fixed_lines.append(code_part)
                # Skip empty lines
            else:
                fixed_lines.append(line)
        else:
            fixed_lines.append(line)
    
    return '\n'.join(fixed_lines)

def process_file(filepath):
    """Process a single Python file."""
    try:
        with open(filepath, 'r', encoding='utf-8') as f:
            content = f.read()
        
        original_content = content
        fixed_content = remove_problematic_comments(content)
        
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
    """Main function to process all Python files."""
    total_fixed = 0
    
    # Process all Python files in the repository
    for root, _, files in os.walk('.'):
        # Skip hidden directories and build directories
        if any(skip in root for skip in ['/.git/', '/target/', '/node_modules/', '/.autoclaude/']):
            continue
        
        for filename in files:
            if filename.endswith('.py'):
                filepath = os.path.join(root, filename)
                if process_file(filepath):
                    total_fixed += 1
    
    print(f"\nTotal files fixed: {total_fixed}")

if __name__ == "__main__":
    main()