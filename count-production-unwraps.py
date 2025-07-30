#!/usr/bin/env python3
"""Count unwraps in actual production code only."""

import os
import re
import glob

def count_unwraps_in_file(file_path):
    """Count unwraps in production code only."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            lines = f.readlines()
        
        unwrap_count = 0
        in_test_block = False
        in_test_mod = False
        
        for i, line in enumerate(lines):
            stripped = line.strip()
            
            # Check for test markers
            if '#[cfg(test)]' in line or '#[test]' in line:
                in_test_block = True
            elif 'mod tests {' in line or 'mod test {' in line:
                in_test_mod = True
            elif in_test_mod and line.strip() == '}' and not any(l.strip() for l in lines[i+1:i+3]):
                # End of test module
                in_test_mod = False
            
            # Skip if in test
            if in_test_block or in_test_mod:
                continue
            
            # Skip if file is in test directory
            if '/tests/' in file_path or '/test/' in file_path or file_path.endswith('_test.rs') or file_path.endswith('_tests.rs'):
                continue
            
            # Count unwraps
            if '.unwrap()' in line and not line.strip().startswith('//'):
                unwrap_count += line.count('.unwrap()')
        
        return unwrap_count
    
    except Exception as e:
        print(f"Error processing {file_path}: {e}")
        return 0

def main():
    """Main function."""
    total_unwraps = 0
    file_unwraps = {}
    
    for pattern in ['crates/**/*.rs', 'node/**/*.rs']:
        for file_path in glob.glob(pattern, recursive=True):
            if os.path.isfile(file_path):
                count = count_unwraps_in_file(file_path)
                if count > 0:
                    file_unwraps[file_path] = count
                    total_unwraps += count
    
    # Sort by unwrap count
    sorted_files = sorted(file_unwraps.items(), key=lambda x: x[1], reverse=True)
    
    print("=== Production Code Unwrap Count ===\n")
    print("Top 20 files with unwraps:")
    for file_path, count in sorted_files[:20]:
        print(f"{count:4d} unwraps in {file_path}")
    
    print(f"\nTotal production unwraps: {total_unwraps}")
    print(f"Total files with unwraps: {len(file_unwraps)}")

if __name__ == '__main__':
    main()