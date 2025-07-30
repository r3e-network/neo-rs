#!/usr/bin/env python3
"""Identify and help fix large functions."""

import os
import re
import glob

def analyze_large_functions(file_path):
    """Analyze functions in a file and identify those over 100 lines."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            lines = f.readlines()
        
        large_functions = []
        in_function = False
        function_start = 0
        function_name = ""
        brace_count = 0
        
        for i, line in enumerate(lines):
            # Detect function start
            if re.match(r'\s*(pub\s+)?(async\s+)?fn\s+(\w+)', line) and '{' in line:
                match = re.search(r'fn\s+(\w+)', line)
                if match:
                    in_function = True
                    function_start = i
                    function_name = match.group(1)
                    brace_count = line.count('{') - line.count('}')
            elif in_function:
                brace_count += line.count('{') - line.count('}')
                if brace_count == 0:
                    # Function ended
                    function_length = i - function_start + 1
                    if function_length > 100:
                        large_functions.append((function_name, function_start + 1, i + 1, function_length))
                    in_function = False
        
        return large_functions
    
    except Exception as e:
        print(f"Error analyzing {file_path}: {e}")
        return []

def suggest_refactoring(file_path, function_info):
    """Suggest refactoring for a large function."""
    function_name, start_line, end_line, length = function_info
    
    suggestions = []
    
    # Common refactoring patterns
    if length > 200:
        suggestions.append(f"CRITICAL: Function '{function_name}' is {length} lines long!")
        suggestions.append("Consider breaking this into multiple smaller functions.")
        suggestions.append("Look for:")
        suggestions.append("  - Repeated code blocks that can be extracted")
        suggestions.append("  - Distinct logical sections that can be separate functions")
        suggestions.append("  - Complex conditions that can be helper functions")
    elif length > 150:
        suggestions.append(f"WARNING: Function '{function_name}' is {length} lines long.")
        suggestions.append("Consider extracting helper functions for:")
        suggestions.append("  - Validation logic")
        suggestions.append("  - Data transformation")
        suggestions.append("  - Error handling blocks")
    else:
        suggestions.append(f"Function '{function_name}' is {length} lines long.")
        suggestions.append("Consider extracting complex logic into helper functions.")
    
    return suggestions

def main():
    """Main function to analyze large functions."""
    all_large_functions = []
    
    # Skip test files
    for pattern in ['crates/**/*.rs', 'node/**/*.rs']:
        for file_path in glob.glob(pattern, recursive=True):
            if os.path.isfile(file_path) and not any(skip in file_path for skip in ['test', '/tests/', '/examples/', 'bench']):
                large_functions = analyze_large_functions(file_path)
                if large_functions:
                    all_large_functions.append((file_path, large_functions))
    
    # Sort by function size
    sorted_functions = []
    for file_path, functions in all_large_functions:
        for func in functions:
            sorted_functions.append((file_path, func))
    
    sorted_functions.sort(key=lambda x: x[1][3], reverse=True)
    
    # Print top 10 largest functions
    print("=== Top 10 Largest Functions ===\n")
    for i, (file_path, func_info) in enumerate(sorted_functions[:10]):
        function_name, start_line, end_line, length = func_info
        print(f"{i+1}. {file_path}:{start_line}")
        print(f"   Function: {function_name}")
        print(f"   Lines: {length} (lines {start_line}-{end_line})")
        
        suggestions = suggest_refactoring(file_path, func_info)
        for suggestion in suggestions:
            print(f"   {suggestion}")
        print()
    
    print(f"\nTotal large functions (>100 lines): {len(sorted_functions)}")
    print("\nRefactoring Strategy:")
    print("1. Start with the largest functions first")
    print("2. Extract helper functions for distinct logical blocks")
    print("3. Create separate modules for complex subsystems")
    print("4. Use traits to share common functionality")
    print("5. Consider using the Builder pattern for complex object construction")

if __name__ == '__main__':
    main()