#!/usr/bin/env python3
import re
import os

# These files have panic in test functions, need different fix
test_panic_fixes = {
    'crates/json/src/jboolean.rs': {
        127: '            _ => panic!("Expected JToken::Boolean"),',
    },
    'crates/json/src/jnumber.rs': {
        257: '            _ => panic!("Expected JToken::Number"),',
    },
    'crates/json/src/jstring.rs': {
        150: '            _ => panic!("Expected JToken::String"),',
    },
    'crates/json/src/jarray.rs': {
        327: '            _ => panic!("Expected JToken::Array"),',
    },
}

def fix_test_panics(file_path, line_fixes):
    """Fix panic statements back in test files."""
    if not os.path.exists(file_path):
        print(f"Warning: {file_path} not found")
        return
    
    with open(file_path, 'r') as f:
        lines = f.readlines()
    
    # Apply fixes
    for line_num, replacement in sorted(line_fixes.items(), reverse=True):
        line_idx = line_num - 1
        if line_idx < len(lines):
            # Preserve indentation
            original = lines[line_idx].rstrip()
            indent = len(original) - len(original.lstrip())
            lines[line_idx] = ' ' * indent + replacement.lstrip() + '\n'
            print(f"Fixed test panic at {file_path}:{line_num}")
    
    # Write back
    with open(file_path, 'w') as f:
        f.writelines(lines)

# Apply all fixes
for file_path, line_fixes in test_panic_fixes.items():
    fix_test_panics(file_path, line_fixes)

print("\nTest panic fixes completed!")