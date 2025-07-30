#!/usr/bin/env python3
"""Remove commented-out code from Rust files."""

import os
import re

def is_documentation_comment(line):
    """Check if a line is a documentation comment."""
    stripped = line.strip()
    return stripped.startswith('///') or stripped.startswith('//!')

def is_commented_code(line):
    """Check if a line looks like commented-out code."""
    stripped = line.strip()
    if not stripped.startswith('//'):
        return False
    
    # Skip documentation comments
    if is_documentation_comment(line):
        return False
    
    # Skip explanatory comments (usually have text descriptions)
    comment_text = stripped[2:].strip()
    
    # Patterns that indicate code rather than comments
    code_patterns = [
        r'^(let|const|pub|fn|struct|impl|use|mod|enum|trait|type|static|extern|unsafe|async|await|return|if|else|match|while|for|loop)\b',
        r'^[a-zA-Z_][a-zA-Z0-9_]*\s*[=\(\{\[]',  # Variable assignment or function call
        r'^[a-zA-Z_][a-zA-Z0-9_]*\.[a-zA-Z_]',   # Method call
        r'^\}|\{|;$',                              # Code block markers
        r'^#\[',                                   # Attributes
        r'^\w+::\w+',                              # Module paths
    ]
    
    for pattern in code_patterns:
        if re.match(pattern, comment_text):
            return True
    
    return False

def remove_commented_code_from_file(filepath):
    """Remove commented-out code from a single file."""
    if not filepath.endswith('.rs'):
        return 0
    
    with open(filepath, 'r') as f:
        lines = f.readlines()
    
    new_lines = []
    removed_count = 0
    i = 0
    
    while i < len(lines):
        line = lines[i]
        
        # Check for block comments containing code
        if '/*' in line and '*/' in line:
            # Single line block comment
            new_lines.append(line)
        elif '/*' in line:
            # Start of multiline block comment
            block_lines = [line]
            i += 1
            while i < len(lines) and '*/' not in lines[i]:
                block_lines.append(lines[i])
                i += 1
            if i < len(lines):
                block_lines.append(lines[i])
            
            # Check if block contains code
            block_content = ''.join(block_lines)
            if any(pattern in block_content for pattern in ['fn ', 'let ', 'impl ', 'struct ']):
                removed_count += len(block_lines)
            else:
                new_lines.extend(block_lines)
        elif is_commented_code(line):
            # Skip this line (commented-out code)
            removed_count += 1
            
            # If the previous line is an empty line and the next line is too, 
            # remove one empty line to avoid double spacing
            if (new_lines and new_lines[-1].strip() == '' and 
                i + 1 < len(lines) and lines[i + 1].strip() == ''):
                pass  # Skip adding extra blank line
        else:
            new_lines.append(line)
        
        i += 1
    
    if removed_count > 0:
        with open(filepath, 'w') as f:
            f.writelines(new_lines)
        print(f"Removed {removed_count} lines of commented code from {filepath}")
    
    return removed_count

def main():
    """Main function to remove commented-out code."""
    total_removed = 0
    
    # Find all Rust files
    for root, dirs, files in os.walk('crates'):
        # Skip test and target directories
        if any(skip in root for skip in ['target', '.git']):
            continue
            
        for file in files:
            if file.endswith('.rs'):
                filepath = os.path.join(root, file)
                total_removed += remove_commented_code_from_file(filepath)
    
    print(f"\nTotal lines of commented code removed: {total_removed}")

if __name__ == "__main__":
    main()