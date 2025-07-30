#!/usr/bin/env python3
"""Fix CamelCase violations in variable names."""

import os
import re
import glob

def to_snake_case(name):
    """Convert CamelCase to snake_case."""
    # Handle acronyms and numbers
    s1 = re.sub('(.)([A-Z][a-z]+)', r'\1_\2', name)
    s2 = re.sub('([a-z0-9])([A-Z])', r'\1_\2', s1)
    return s2.lower()

def fix_camelcase_in_file(file_path):
    """Fix CamelCase violations in a file."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        original_content = content
        changes_made = 0
        
        # Skip test files
        if any(skip in file_path for skip in ['test', '/tests/', '/examples/', 'bench']):
            return 0
        
        # Common CamelCase patterns in Rust that should be snake_case
        
        # 1. let/const variable declarations
        let_pattern = r'\b(let|const)\s+(?:mut\s+)?([a-z][a-zA-Z0-9]+)\s*[:=]'
        for match in re.finditer(let_pattern, content):
            var_name = match.group(2)
            # Check if it's CamelCase
            if any(c.isupper() for c in var_name[1:]):
                snake_name = to_snake_case(var_name)
                # Replace all occurrences of this variable
                content = re.sub(r'\b' + var_name + r'\b', snake_name, content)
                changes_made += 1
        
        # 2. Function parameters
        fn_param_pattern = r'fn\s+\w+\s*[<\w,\s>]*\s*\(([^)]+)\)'
        for match in re.finditer(fn_param_pattern, content):
            params = match.group(1)
            # Parse parameters
            param_pattern = r'(?:mut\s+)?(\w+)\s*:\s*[^,)]+'
            for param_match in re.finditer(param_pattern, params):
                param_name = param_match.group(1)
                if param_name != 'self' and any(c.isupper() for c in param_name):
                    snake_name = to_snake_case(param_name)
                    # Replace in function body (approximate)
                    fn_end = content.find('{', match.end())
                    if fn_end != -1:
                        # Find the matching closing brace
                        brace_count = 1
                        i = fn_end + 1
                        while i < len(content) and brace_count > 0:
                            if content[i] == '{':
                                brace_count += 1
                            elif content[i] == '}':
                                brace_count -= 1
                            i += 1
                        
                        if i <= len(content):
                            fn_body = content[fn_end:i]
                            new_fn_body = re.sub(r'\b' + param_name + r'\b', snake_name, fn_body)
                            if fn_body != new_fn_body:
                                content = content[:fn_end] + new_fn_body + content[i:]
                                changes_made += 1
        
        # 3. Struct field names
        struct_pattern = r'struct\s+\w+\s*(?:<[^>]+>)?\s*\{([^}]+)\}'
        for match in re.finditer(struct_pattern, content, re.DOTALL):
            fields = match.group(1)
            field_pattern = r'(?:pub\s+)?(\w+)\s*:\s*[^,\n}]+'
            for field_match in re.finditer(field_pattern, fields):
                field_name = field_match.group(1)
                if any(c.isupper() for c in field_name):
                    snake_name = to_snake_case(field_name)
                    # This is more complex - would need to update all usages
                    
                    pass
        
        # 4. Common specific replacements
        common_replacements = [
            (r'\bbaseUrl\b', 'base_url'),
            (r'\bmaxSize\b', 'max_size'),
            (r'\bminSize\b', 'min_size'),
            (r'\btotalCount\b', 'total_count'),
            (r'\bcurrentIndex\b', 'current_index'),
            (r'\blastUpdate\b', 'last_update'),
            (r'\bisActive\b', 'is_active'),
            (r'\bhasMore\b', 'has_more'),
            (r'\btimeStamp\b', 'timestamp'),
            (r'\bdataSize\b', 'data_size'),
            (r'\bbyteArray\b', 'byte_array'),
            (r'\bstartTime\b', 'start_time'),
            (r'\bendTime\b', 'end_time'),
            (r'\btxCount\b', 'tx_count'),
            (r'\blockSize\b', 'lock_size'),
            (r'\bprevHash\b', 'prev_hash'),
            (r'\bnextHash\b', 'next_hash'),
        ]
        
        for old_name, new_name in common_replacements:
            if re.search(old_name, content):
                content = re.sub(old_name, new_name, content)
                changes_made += 1
        
        if content != original_content:
            with open(file_path, 'w', encoding='utf-8') as f:
                f.write(content)
            return changes_made
        
        return 0
    
    except Exception as e:
        print(f"Error processing {file_path}: {e}")
        return 0

def main():
    """Main function to fix CamelCase violations."""
    total_fixes = 0
    files_fixed = 0
    
    # Process all Rust files
    for pattern in ['crates/**/*.rs', 'node/**/*.rs']:
        for file_path in glob.glob(pattern, recursive=True):
            if os.path.isfile(file_path):
                fixes = fix_camelcase_in_file(file_path)
                if fixes > 0:
                    print(f"Fixed {fixes} CamelCase violations in {file_path}")
                    total_fixes += fixes
                    files_fixed += 1
    
    print(f"\nTotal CamelCase violations fixed: {total_fixes} in {files_fixed} files")

if __name__ == '__main__':
    main()