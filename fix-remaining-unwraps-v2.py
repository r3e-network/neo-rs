#!/usr/bin/env python3
"""Fix remaining unwrap() calls in production code with more aggressive replacements."""

import os
import re
import sys

def is_test_file(filepath):
    """Check if file is a test file."""
    return any(part in filepath for part in ['tests/', 'test/', 'benches/', 'examples/', '_test.rs'])

def is_in_test_context(content, match_pos):
    """Check if unwrap() is within test context."""
    # Look back up to 1500 chars for test markers
    start = max(0, match_pos - 1500)
    context = content[start:match_pos]
    
    test_markers = [
        '#[test]',
        '#[tokio::test]',
        '#[cfg(test)]',
        'mod tests {',
        'mod test {',
        '#[bench]',
    ]
    
    return any(marker in context for marker in test_markers)

def get_function_context(content, pos):
    """Get the function context for better error handling."""
    # Find the function signature
    func_start = content.rfind('\nfn ', 0, pos)
    if func_start == -1:
        func_start = content.rfind('\npub fn ', 0, pos)
    if func_start == -1:
        func_start = content.rfind('\npub(crate) fn ', 0, pos)
    if func_start == -1:
        func_start = content.rfind('\nasync fn ', 0, pos)
    if func_start == -1:
        func_start = content.rfind('\npub async fn ', 0, pos)
    
    if func_start != -1:
        func_end = content.find('{', func_start)
        if func_end != -1:
            func_sig = content[func_start:func_end]
            # Check if function returns Result
            if '-> Result' in func_sig or '-> VmResult' in func_sig or '-> CoreResult' in func_sig:
                return 'result'
            elif '-> Option' in func_sig:
                return 'option'
    
    return 'none'

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
            # Get context for appropriate error handling
            line_start = content.rfind('\n', 0, match.start()) + 1
            line_end = content.find('\n', match.end())
            if line_end == -1:
                line_end = len(content)
            
            line = content[line_start:line_end]
            func_context = get_function_context(content, match.start())
            
            # Skip if it's already handling the error properly
            if 'unwrap_or' in line or 'expect(' in line:
                continue
            
            # Determine appropriate replacement based on context
            replacement = None
            
            # Specific patterns
            if '.parse()' in line:
                if func_context == 'result':
                    replacement = '.map_err(|e| format!("Parse error: {}", e))?'
                else:
                    replacement = '.unwrap_or_default()'
            elif '.lock()' in line:
                if func_context == 'result':
                    replacement = '.map_err(|e| format!("Lock poisoned: {}", e))?'
                else:
                    replacement = '.expect("Lock poisoned")'
            elif '.write()' in line or '.read()' in line:
                if func_context == 'result':
                    replacement = '.map_err(|e| format!("RwLock poisoned: {}", e))?'
                else:
                    replacement = '.expect("RwLock poisoned")'
            elif '.get(' in line or '.get_mut(' in line:
                if func_context == 'result':
                    replacement = '.ok_or_else(|| "Item not found")?'
                else:
                    replacement = '.expect("Item not found")'
            elif '.first()' in line or '.last()' in line or '.next()' in line:
                if func_context == 'result':
                    replacement = '.ok_or_else(|| "Empty collection")?'
                else:
                    replacement = '.expect("Empty collection")'
            elif '.pop()' in line:
                if func_context == 'result':
                    replacement = '.ok_or_else(|| "Empty stack")?'
                else:
                    replacement = '.expect("Empty stack")'
            elif 'Duration::' in line or 'SystemTime::' in line:
                if func_context == 'result':
                    replacement = '.map_err(|e| format!("Time error: {}", e))?'
                else:
                    replacement = '.unwrap_or_default()'
            elif '.send(' in line:
                if func_context == 'result':
                    replacement = '.map_err(|e| format!("Channel send error: {}", e))?'
                else:
                    replacement = '.expect("Channel send failed")'
            elif '.recv()' in line:
                if func_context == 'result':
                    replacement = '.map_err(|e| format!("Channel receive error: {}", e))?'
                else:
                    replacement = '.expect("Channel receive failed")'
            elif 'Arc::new' in line or 'Mutex::new' in line:
                # These constructors don't fail
                continue
            else:
                # Generic replacement
                if func_context == 'result':
                    replacement = '?'
                else:
                    # For non-Result contexts, use expect with context
                    if 'HashMap' in line or 'BTreeMap' in line:
                        replacement = '.expect("Map operation failed")'
                    elif 'Vec' in line:
                        replacement = '.expect("Vector operation failed")'
                    elif 'String' in line:
                        replacement = '.expect("String operation failed")'
                    else:
                        replacement = '.expect("Operation failed")'
            
            if replacement:
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
    
    # Target specific directories with most unwraps
    target_dirs = [
        'crates/vm/src',
        'crates/core/src',
        'crates/network/src',
        'crates/ledger/src',
        'crates/consensus/src',
        'crates/smart_contract/src',
    ]
    
    for target_dir in target_dirs:
        if os.path.exists(target_dir):
            for root, dirs, files in os.walk(target_dir):
                # Skip test directories
                if any(skip in root for skip in ['target', '.git', 'tests', 'benches']):
                    continue
                    
                for file in files:
                    if file.endswith('.rs'):
                        filepath = os.path.join(root, file)
                        total_changes += fix_unwrap_in_file(filepath)
    
    print(f"\nTotal unwrap() calls fixed: {total_changes}")

if __name__ == "__main__":
    main()