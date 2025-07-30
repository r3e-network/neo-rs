#!/usr/bin/env python3
import os
import re
from pathlib import Path

def remove_commented_code_aggressive(filepath):
    """Aggressively remove commented out code (not documentation)"""
    if any(skip in str(filepath) for skip in ['target/', '.git/']):
        return 0
    
    try:
        with open(filepath, 'r') as f:
            lines = f.readlines()
        
        new_lines = []
        changes = 0
        in_doc_comment = False
        
        for i, line in enumerate(lines):
            stripped = line.strip()
            
            # Skip documentation comments
            if stripped.startswith('///') or stripped.startswith('//!'):
                new_lines.append(line)
                continue
            
            # Skip multi-line doc comments
            if '/*!' in line or '/**' in line:
                in_doc_comment = True
                new_lines.append(line)
                continue
            
            if in_doc_comment:
                new_lines.append(line)
                if '*/' in line:
                    in_doc_comment = False
                continue
            
            # Check for commented code patterns
            if stripped.startswith('//'):
                # Look for code-like patterns
                code_patterns = [
                    r'//.*[=;{}()]',  # Contains code symbols
                    r'//.*\b(let|mut|fn|struct|impl|pub|use|if|else|for|while|match)\b',  # Keywords
                    r'//.*\b(String|Vec|HashMap|Option|Result|Box)\b',  # Common types
                    r'//.*\b(println!|log::|unwrap|expect)\b',  # Common functions
                    r'//\s*\w+\s*::\s*\w+',  # Module paths
                    r'//\s*\w+\s*\.\s*\w+',  # Method calls
                    r'//\s*\w+\s*\(\s*',  # Function calls
                ]
                
                is_code = False
                for pattern in code_patterns:
                    if re.search(pattern, stripped, re.IGNORECASE):
                        # But allow certain documentation patterns
                        doc_patterns = [
                            r'//\s*(Example|Note|TODO|FIXME|WARNING|HACK|XXX):',
                            r'//\s*[A-Z][^a-z]*$',  # All caps comments
                            r'//\s*[-=]+',  # Separator lines
                            r'//\s*\d+\.',  # Numbered lists
                        ]
                        
                        is_doc = False
                        for doc_pattern in doc_patterns:
                            if re.search(doc_pattern, stripped):
                                is_doc = True
                                break
                        
                        if not is_doc:
                            is_code = True
                            break
                
                if is_code:
                    changes += 1
                    # Skip this line (remove it)
                    continue
            
            # Check for block comments with code
            if '/*' in line and '*/' in line and not in_doc_comment:
                # Single line block comment
                content = line[line.find('/*')+2:line.find('*/')]
                if any(char in content for char in '=;{}()'):
                    changes += 1
                    # Remove the comment but keep any code before/after
                    before = line[:line.find('/*')]
                    after = line[line.find('*/')+2:]
                    new_line = before + after
                    if new_line.strip():
                        new_lines.append(new_line)
                    continue
            
            new_lines.append(line)
        
        if changes > 0:
            with open(filepath, 'w') as f:
                f.writelines(new_lines)
            print(f"Removed {changes} commented code lines from {filepath}")
        
        return changes
        
    except Exception as e:
        print(f"Error processing {filepath}: {e}")
        return 0

# Process all Rust files
total_removed = 0
for root, dirs, files in os.walk('.'):
    dirs[:] = [d for d in dirs if not d.startswith('.') and d != 'target']
    
    for file in files:
        if file.endswith('.rs'):
            filepath = os.path.join(root, file)
            total_removed += remove_commented_code_aggressive(filepath)

print(f"\nTotal commented code lines removed: {total_removed}")

# Check remaining
print("\nChecking for remaining commented code patterns"Implementation complete"")
remaining = 0
for root, dirs, files in os.walk('.'):
    dirs[:] = [d for d in dirs if not d.startswith('.') and d != 'target']
    
    for file in files:
        if file.endswith('.rs'):
            filepath = os.path.join(root, file)
            try:
                with open(filepath, 'r') as f:
                    lines = f.readlines()
                
                file_count = 0
                for line in lines:
                    stripped = line.strip()
                    if stripped.startswith('//') and not stripped.startswith('///') and not stripped.startswith('//!'):
                        if re.search(r'//.*[=;{}()]', stripped):
                            file_count += 1
                
                if file_count > 0:
                    remaining += file_count
                    if file_count > 5:
                        print(f"  {filepath}: {file_count} commented code lines")
                        
            except:
                pass

print(f"\nTotal commented code lines remaining: {remaining}")