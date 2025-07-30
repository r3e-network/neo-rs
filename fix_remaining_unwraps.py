#!/usr/bin/env python3
import os
import re
from pathlib import Path

def should_skip_file(filepath):
    """Skip test files and examples"""
    skip_patterns = ['test', 'example', 'bench', 'mock']
    return any(pattern in str(filepath).lower() for pattern in skip_patterns)

def fix_unwrap_in_line(line, in_function_returning_result=True):
    """Fix unwrap calls in a line of code"""
    # Don't modify if it's already using unwrap_or variants
    if 'unwrap_or' in line:
        return line
        
    # SystemTime pattern
    if 'duration_since' in line and 'UNIX_EPOCH' in line and '.unwrap()' in line:
        return line.replace('.unwrap()', '.unwrap_or_default()')
    
    # Parse patterns
    if '.parse()' in line and '.unwrap()' in line and in_function_returning_result:
        return line.replace('.unwrap()', '?')
    
    # Lock patterns
    if any(pattern in line for pattern in ['.lock()', '.read()', '.write()']) and '.unwrap()' in line:
        if in_function_returning_result:
            return line.replace('.unwrap()', '.map_err(|_| Error::LockError)?')
    
    # Generic unwrap replacement for functions returning Result
    if in_function_returning_result and '.unwrap()' in line:
        # Check if it's on an Option or Result based on context
        if 'Some(' in line or 'None' in line or '.get(' in line or '.get_mut(' in line:
            # Likely an Option
            return line.replace('.unwrap()', '.ok_or_else(|| Error::MissingValue)?')
        else:
            # Likely a Result
            return line.replace('.unwrap()', '?')
    
    return line

def process_file(filepath):
    """Process a single Rust file"""
    if should_skip_file(filepath):
        return 0
    
    try:
        with open(filepath, 'r') as f:
            content = f.read()
        
        if '.unwrap()' not in content:
            return 0
        
        lines = content.split('\n')
        modified_lines = []
        changes = 0
        in_result_function = False
        
        for line in lines:
            # Simple heuristic to detect if we're in a function returning Result
            if 'fn ' in line and '-> Result' in line:
                in_result_function = True
            elif 'fn ' in line:
                in_result_function = False
            
            if '.unwrap()' in line and not any(skip in line for skip in ['test', 'example', '//']):
                new_line = fix_unwrap_in_line(line, in_result_function)
                if new_line != line:
                    changes += 1
                    modified_lines.append(new_line)
                else:
                    modified_lines.append(line)
            else:
                modified_lines.append(line)
        
        if changes > 0:
            # Write back the file
            with open(filepath, 'w') as f:
                f.write('\n'.join(modified_lines))
            print(f"Fixed {changes} unwrap() calls in {filepath}")
        
        return changes
    except Exception as e:
        print(f"Error processing {filepath}: {e}")
        return 0

# Main execution
total_changes = 0
for root, dirs, files in os.walk('.'):
    # Skip hidden directories and target
    dirs[:] = [d for d in dirs if not d.startswith('.') and d != 'target']
    
    for file in files:
        if file.endswith('.rs'):
            filepath = os.path.join(root, file)
            total_changes += process_file(filepath)

print(f"\nTotal unwrap() calls fixed: {total_changes}")
