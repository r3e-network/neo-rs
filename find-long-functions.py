#!/usr/bin/env python3
import os
import re
from pathlib import Path

def find_long_functions(file_path, min_lines=100):
    """Find functions longer than min_lines in a Rust file."""
    with open(file_path, 'r') as f:
        lines = f.readlines()
    
    results = []
    in_function = False
    function_start = 0
    function_name = ""
    brace_count = 0
    
    for i, line in enumerate(lines):
        # Look for function definitions
        if re.match(r'\s*(pub\s+)?(async\s+)?fn\s+\w+', line):
            if '{' in line:
                in_function = True
                function_start = i
                function_name = line.strip()
                brace_count = line.count('{') - line.count('}')
        elif in_function:
            brace_count += line.count('{') - line.count('}')
            if brace_count == 0:
                function_length = i - function_start + 1
                if function_length >= min_lines:
                    results.append({
                        'file': file_path,
                        'function': function_name,
                        'start': function_start + 1,
                        'end': i + 1,
                        'length': function_length
                    })
                in_function = False
    
    return results

# Search for Rust files
long_functions = []
for root, dirs, files in os.walk('.'):
    # Skip target and other build directories
    if any(skip in root for skip in ['/target/', '/.git/', '/node_modules/', '/.idea/']):
        continue
    
    for file in files:
        if file.endswith('.rs'):
            file_path = os.path.join(root, file)
            try:
                results = find_long_functions(file_path)
                long_functions.extend(results)
            except Exception as e:
                pass

# Sort by length descending
long_functions.sort(key=lambda x: x['length'], reverse=True)

# Print results
print("=== Long Functions (>100 lines) ===")
print()
for func in long_functions[:10]:  # Show top 10
    print(f"File: {func['file']}")
    print(f"Function: {func['function']}")
    print(f"Lines: {func['start']}-{func['end']} ({func['length']} lines)")
    print()