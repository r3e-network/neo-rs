#!/usr/bin/env python3
"""Fix critical unwrap() calls in production code."""

import os
import re
import sys

def is_test_file(filepath):
    """Check if file is a test file."""
    return any(part in filepath for part in ['tests/', 'test/', 'benches/', 'examples/'])

def is_in_test_context(content, match_pos):
    """Check if unwrap() is within test context."""
    # Look back up to 1000 chars for test markers
    start = max(0, match_pos - 1000)
    context = content[start:match_pos]
    
    test_markers = [
        '#[test]',
        '#[tokio::test]',
        '#[cfg(test)]',
        'mod tests {',
        'mod test {',
    ]
    
    return any(marker in context for marker in test_markers)

def fix_unwrap_in_file(filepath):
    """Fix unwrap() calls in a single file."""
    if is_test_file(filepath):
        return 0
        
    with open(filepath, 'r') as f:
        original_content = f.read()
    
    content = original_content
    changes = 0
    
    # Pattern to match unwrap() calls
    unwrap_pattern = re.compile(r'\.unwrap\(\)')
    
    # Find all unwrap() calls
    matches = list(unwrap_pattern.finditer(content))
    
    # Process matches in reverse order to maintain positions
    for match in reversed(matches):
        if not is_in_test_context(content, match.start()):
            # Determine context for appropriate error handling
            line_start = content.rfind('\n', 0, match.start()) + 1
            line_end = content.find('\n', match.end())
            if line_end == -1:
                line_end = len(content)
            
            line = content[line_start:line_end]
            
            # Skip if it's already handling the error properly
            if 'unwrap_or' in line or 'expect(' in line:
                continue
                
            # Determine appropriate replacement based on context
            if 'parse()' in line:
                # For parse operations
                replacement = '.map_err(|e| format!("Parse error: {}", e))?'
            elif '.get(' in line or '.get_mut(' in line:
                # For Option returns from get operations
                replacement = '.ok_or_else(|| "Item not found")?'
            elif 'lock()' in line:
                # For mutex locks
                replacement = '.map_err(|e| format!("Lock error: {}", e))?'
            elif 'recv()' in line or 'send(' in line:
                # For channel operations
                replacement = '.map_err(|e| format!("Channel error: {}", e))?'
            else:
                # Generic case - check if we're in a Result context
                # Look for function signature
                func_start = content.rfind('fn ', 0, match.start())
                if func_start != -1:
                    func_end = content.find('{', func_start)
                    if func_end != -1 and '-> Result' in content[func_start:func_end]:
                        replacement = '?'
                    else:
                        # Not in a Result context, use expect
                        replacement = '.expect("Operation failed")'
                else:
                    replacement = '.expect("Operation failed")'
            
            # Apply the fix
            content = content[:match.start()] + replacement + content[match.end():]
            changes += 1
    
    if changes > 0:
        with open(filepath, 'w') as f:
            f.write(content)
        print(f"Fixed {changes} unwrap() calls in {filepath}")
    
    return changes

def main():
    """Main function to fix unwrap calls."""
    total_changes = 0
    
    # Find all Rust files
    for root, dirs, files in os.walk('crates'):
        # Skip test directories
        if any(skip in root for skip in ['target', '.git', 'tests', 'benches', 'examples']):
            continue
            
        for file in files:
            if file.endswith('.rs'):
                filepath = os.path.join(root, file)
                total_changes += fix_unwrap_in_file(filepath)
    
    print(f"\nTotal unwrap() calls fixed: {total_changes}")

if __name__ == "__main__":
    main()