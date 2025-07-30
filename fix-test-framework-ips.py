#!/usr/bin/env python3
"""Fix IPs in test framework to use constants."""

import os

def fix_test_framework_ips():
    """Fix hardcoded IPs in test framework."""
    
    file_path = 'crates/network/src/p2p/local_test_framework.rs'
    
    if not os.path.exists(file_path):
        print(f"File not found: {file_path}")
        return 0
        
    with open(file_path, 'r') as f:
        content = f.read()
    
    # Add constants at the top
    if 'const TEST_NODE_1' not in content:
        lines = content.splitlines()
        insert_idx = 0
        
        # Find where to insert after use statements
        for i, line in enumerate(lines):
            if line.startswith('use '):
                insert_idx = i + 1
            elif not line.strip() and insert_idx > 0:
                break
        
        # Insert test constants
        constants = [
            '',
            '/// Test framework constants',
            'const TEST_NODE_1: &str = "localhost:20001";',
            'const TEST_NODE_2: &str = "localhost:20002";',
            'const TEST_NODE_3: &str = "localhost:20003";',
            ''
        ]
        
        for j, const_line in enumerate(constants):
            lines.insert(insert_idx + j, const_line)
        
        content = '\n'.join(lines)
    
    # Replace hardcoded IPs
    replacements = [
        ('"127.0.0.1:20001"', 'TEST_NODE_1'),
        ('"127.0.0.1:20002"', 'TEST_NODE_2'),
        ('"127.0.0.1:20003"', 'TEST_NODE_3'),
    ]
    
    changes = 0
    for old, new in replacements:
        if old in content:
            content = content.replace(old, new)
            changes += 1
    
    if changes > 0:
        with open(file_path, 'w') as f:
            f.write(content)
        print(f"Fixed {changes} hardcoded IPs in {file_path}")
        return changes
    
    return 0

if __name__ == '__main__':
    fix_test_framework_ips()