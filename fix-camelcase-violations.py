#!/usr/bin/env python3
"""Fix CamelCase variable naming violations to snake_case."""

import os
import re
import sys

def camel_to_snake(name):
    """Convert CamelCase to snake_case."""
    # Insert underscore before uppercase letters that follow lowercase letters
    s1 = re.sub('(.)([A-Z][a-z]+)', r'\1_\2', name)
    # Insert underscore before uppercase letters that follow lowercase or uppercase letters
    return re.sub('([a-z0-9])([A-Z])', r'\1_\2', s1).lower()

def is_camel_case(name):
    """Check if name is in CamelCase (not CONSTANT_CASE)."""
    # Skip if all uppercase (likely a constant)
    if name.isupper():
        return False
    # Skip if already snake_case
    if '_' in name:
        return False
    # Check if it has uppercase letters after the first character
    return bool(re.search(r'[a-z][A-Z]', name))

def fix_camelcase_in_file(filepath):
    """Fix CamelCase violations in a single file."""
    if not filepath.endswith('.rs'):
        return 0
        
    with open(filepath, 'r') as f:
        content = f.read()
    
    original_content = content
    changes = 0
    
    # Pattern to match variable declarations
    # let VariableName = ...
    # let mut VariableName = ...
    let_pattern = re.compile(r'\blet\s+(mut\s+)?([A-Z][a-zA-Z0-9]*)\b')
    
    # Pattern to match function parameters
    # fn function_name(VariableName: Type)
    param_pattern = re.compile(r'(\w+\s*:\s*[^,\)]+)')
    
    # Pattern to match struct fields
    # field_name: Type,
    field_pattern = re.compile(r'^\s*([A-Z][a-zA-Z0-9]*)\s*:\s*[^,]+,?\s*$', re.MULTILINE)
    
    # Fix let bindings
    for match in let_pattern.finditer(content):
        var_name = match.group(2)
        if is_camel_case(var_name):
            snake_name = camel_to_snake(var_name)
            # Replace the declaration
            old_decl = match.group(0)
            new_decl = old_decl.replace(var_name, snake_name)
            content = content.replace(old_decl, new_decl)
            
            # Replace all uses of this variable in the scope
            # This is a simple approach - might need refinement
            content = re.sub(r'\b' + var_name + r'\b', snake_name, content)
            changes += 1
    
    # Fix struct fields (more conservative)
    lines = content.split('\n')
    in_struct = False
    new_lines = []
    
    for line in lines:
        if 'struct ' in line and '{' in line:
            in_struct = True
        elif '}' in line and in_struct:
            in_struct = False
            
        if in_struct:
            match = re.match(r'^(\s*)([A-Z][a-zA-Z0-9]*)\s*:\s*(.+)$', line)
            if match and is_camel_case(match.group(2)):
                indent = match.group(1)
                field_name = match.group(2)
                field_type = match.group(3)
                snake_name = camel_to_snake(field_name)
                line = f"{indent}{snake_name}: {field_type}"
                changes += 1
        
        new_lines.append(line)
    
    if changes > 0:
        content = '\n'.join(new_lines)
        with open(filepath, 'w') as f:
            f.write(content)
        print(f"Fixed {changes} CamelCase violations in {filepath}")
    
    return changes

def main():
    """Main function to fix CamelCase violations."""
    total_changes = 0
    
    # Find all Rust files
    for root, dirs, files in os.walk('crates'):
        # Skip test and target directories
        if any(skip in root for skip in ['target', '.git', 'tests', 'benches']):
            continue
            
        for file in files:
            if file.endswith('.rs'):
                filepath = os.path.join(root, file)
                # Skip test files
                if 'test' in filepath:
                    continue
                total_changes += fix_camelcase_in_file(filepath)
    
    print(f"\nTotal CamelCase violations fixed: {total_changes}")

if __name__ == "__main__":
    main()