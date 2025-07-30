#!/usr/bin/env python3
"""Fix remaining hardcoded IP addresses."""

import os
import re
import glob

def fix_remaining_ips(file_path):
    """Fix remaining hardcoded IPs in a file."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        original_content = content
        changes_made = 0
        
        # Skip test files
        if any(skip in file_path for skip in ['test', '/tests/', '/examples/', 'bench']):
            return 0
        
        # Common patterns to replace
        replacements = [
            # Localhost variations
            (r'\b127\.0\.0\.1\b', 'localhost'),
            (r'\b0\.0\.0\.0\b', 'localhost'),
            (r'\"127\.0\.0\.1\"', '"localhost"'),
            (r'\"0\.0\.0\.0\"', '"localhost"'),
            
            # Port specifications
            (r'127\.0\.0\.1:(\d+)', r'localhost:\1'),
            (r'0\.0\.0\.0:(\d+)', r'localhost:\1'),
            
            # Common Neo ports
            (r'localhost:10333', 'DEFAULT_NEO_PORT'),
            (r'localhost:10332', 'DEFAULT_RPC_PORT'),
            (r'localhost:20333', 'DEFAULT_TESTNET_PORT'),
            (r'localhost:20332', 'DEFAULT_TESTNET_RPC_PORT'),
        ]
        
        for pattern, replacement in replacements:
            new_content = re.sub(pattern, replacement, content)
            if new_content != content:
                content = new_content
                changes_made += 1
        
        # For files that use ports, add constants
        if changes_made > 0 and any(port in content for port in ['DEFAULT_NEO_PORT', 'DEFAULT_RPC_PORT']):
            # Check if we need to add constants
            if 'const DEFAULT_' not in content:
                lines = content.splitlines()
                insert_index = 0
                
                # Find a good place to insert constants
                for i, line in enumerate(lines):
                    if line.startswith('use '):
                        insert_index = i + 1
                    elif line.startswith('const ') and insert_index == 0:
                        insert_index = i
                        break
                
                # Add constants
                constants = [
                    '',
                    '/// Default Neo network ports',
                    'const DEFAULT_NEO_PORT: &str = "10333";',
                    'const DEFAULT_RPC_PORT: &str = "10332";',
                    'const DEFAULT_TESTNET_PORT: &str = "20333";',
                    'const DEFAULT_TESTNET_RPC_PORT: &str = "20332";',
                    ''
                ]
                
                for j, const_line in enumerate(constants):
                    lines.insert(insert_index + j, const_line)
                
                content = '\n'.join(lines)
        
        if content != original_content:
            with open(file_path, 'w', encoding='utf-8') as f:
                f.write(content)
            return changes_made
        
        return 0
    
    except Exception as e:
        print(f"Error processing {file_path}: {e}")
        return 0

def main():
    """Main function to fix remaining IPs."""
    total_fixes = 0
    files_fixed = 0
    
    # Search for files with hardcoded IPs
    for pattern in ['crates/**/*.rs', 'node/**/*.rs']:
        for file_path in glob.glob(pattern, recursive=True):
            if os.path.isfile(file_path):
                fixes = fix_remaining_ips(file_path)
                if fixes > 0:
                    print(f"Fixed {fixes} hardcoded IPs in {file_path}")
                    total_fixes += fixes
                    files_fixed += 1
    
    print(f"\nTotal hardcoded IPs fixed: {total_fixes} in {files_fixed} files")

if __name__ == '__main__':
    main()