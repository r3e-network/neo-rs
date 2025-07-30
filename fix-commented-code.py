#!/usr/bin/env python3
"""Remove commented out code from the codebase."""

import os
import re
import glob

def is_valid_comment(comment):
    """Check if a comment is a valid documentation/explanation comment."""
    comment = comment.strip()
    
    # Valid comment patterns
    valid_patterns = [
        r'^//\s*[A-Z]',  # Starts with capital letter (likely documentation)
        r'^//\s*\d+\.',  # Numbered list
        r'^//\s*-\s',    # Bullet point
        r'^//\s*\*\s',   # Bullet point with asterisk
        r'^//\s*TODO',   
        r'^//\s*FIXME',  
        r'^//\s*NOTE',   # NOTE comment
        r'^//\s*HACK',   
        r'^//\s*WARNING', # WARNING comment
        r'^//!',         # Doc comment
        r'^///',         # Doc comment
        r'^//\s*```',    # Code block in documentation
        r'^//\s*Example', # Example documentation
        r'^//\s*Returns', # Returns documentation
        r'^//\s*#\s',    # Markdown header
        r'^//\s*\|',     # Table in documentation
        r'^//\s*>',      # Quote in documentation
    ]
    
    for pattern in valid_patterns:
        if re.match(pattern, comment):
            return True
    
    # Check if it looks like commented out code
    code_patterns = [
        r'^//\s*use\s+',
        r'^//\s*let\s+',
        r'^//\s*fn\s+',
        r'^//\s*pub\s+',
        r'^//\s*struct\s+',
        r'^//\s*enum\s+',
        r'^//\s*impl\s+',
        r'^//\s*if\s+',
        r'^//\s*for\s+',
        r'^//\s*while\s+',
        r'^//\s*match\s+',
        r'^//\s*return\s+',
        r'^//\s*\w+\(',  # Function call
        r'^//\s*\w+::', # Module path
        r'^//\s*[{}();]', # Just brackets or semicolons
        r'^//\s*\w+\s*=\s*', # Assignment
        r'^//\s*\.\w+', # Method call
        r'^//\s*&',     # Reference
        r'^//\s*\*',    # Dereference (not bullet point)
    ]
    
    for pattern in code_patterns:
        if re.match(pattern, comment):
            return False
    
    # If it's a very short comment with no clear purpose, it might be code
    if len(comment) < 10 and not re.match(r'^//\s*[A-Z]', comment):
        return False
    
    return True

def remove_commented_code(file_path):
    """Remove commented out code from a file."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            lines = f.readlines()
        
        new_lines = []
        i = 0
        changes_made = 0
        in_doc_comment = False
        
        while i < len(lines):
            line = lines[i]
            stripped = line.strip()
            
            # Skip doc comments
            if stripped.startswith('///') or stripped.startswith('//!'):
                new_lines.append(line)
                i += 1
                continue
            
            # Handle multi-line comments
            if '/*' in line:
                in_doc_comment = True
                new_lines.append(line)
                i += 1
                continue
            
            if '*/' in line:
                in_doc_comment = False
                new_lines.append(line)
                i += 1
                continue
            
            if in_doc_comment:
                new_lines.append(line)
                i += 1
                continue
            
            # Check for single-line comments
            if stripped.startswith('//'):
                if is_valid_comment(stripped):
                    new_lines.append(line)
                else:
                    # This looks like commented out code - remove it
                    changes_made += 1
                    # Don't add the line
            else:
                # Check for inline commented code
                if '//' in line:
                    # Split at comment
                    code_part = line[:line.index('//')]
                    comment_part = line[line.index('//'):]
                    
                    # Check if the comment part is valid
                    if is_valid_comment(comment_part):
                        new_lines.append(line)
                    else:
                        # Remove the comment part but keep the code
                        new_lines.append(code_part.rstrip() + '\n')
                        changes_made += 1
                else:
                    new_lines.append(line)
            
            i += 1
        
        if changes_made > 0:
            # Remove any resulting empty line clusters (more than 2 consecutive empty lines)
            final_lines = []
            empty_count = 0
            
            for line in new_lines:
                if line.strip() == '':
                    empty_count += 1
                    if empty_count <= 2:
                        final_lines.append(line)
                else:
                    empty_count = 0
                    final_lines.append(line)
            
            with open(file_path, 'w', encoding='utf-8') as f:
                f.writelines(final_lines)
            
            return changes_made
        
        return 0
    
    except Exception as e:
        print(f"Error processing {file_path}: {e}")
        return 0

def main():
    """Main function to remove commented code."""
    total_changes = 0
    files_modified = 0
    
    # Skip test files and examples
    for pattern in ['crates/**/*.rs', 'node/**/*.rs']:
        for file_path in glob.glob(pattern, recursive=True):
            if os.path.isfile(file_path):
                if any(skip in file_path for skip in ['test', '/tests/', '/examples/', 'bench']):
                    continue
                
                changes = remove_commented_code(file_path)
                if changes > 0:
                    print(f"Removed {changes} commented code lines from {file_path}")
                    total_changes += changes
                    files_modified += 1
    
    print(f"\nTotal commented code lines removed: {total_changes} from {files_modified} files")

if __name__ == '__main__':
    main()