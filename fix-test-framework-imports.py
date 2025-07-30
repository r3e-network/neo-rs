#!/usr/bin/env python3
"""Fix duplicate imports in test framework."""

import os
import re

def fix_imports():
    file_path = 'crates/network/src/p2p/local_test_framework.rs'
    
    if not os.path.exists(file_path):
        return 0
        
    with open(file_path, 'r') as f:
        content = f.read()
    
    # Remove duplicate MILLISECONDS_PER_BLOCK imports
    content = re.sub(r'use crate::constants::MILLISECONDS_PER_BLOCK;', '', content)
    
    # Fix the actual hardcoded IPs that remain
    if '"127.0.0.1:' in content:
        content = content.replace('"127.0.0.1:20001"', 'TEST_NODE_1')
        content = content.replace('"127.0.0.1:20002"', 'TEST_NODE_2') 
        content = content.replace('"127.0.0.1:20003"', 'TEST_NODE_3')
    
    with open(file_path, 'w') as f:
        f.write(content)
    
    print("Fixed imports and remaining IPs")
    return 1

if __name__ == '__main__':
    fix_imports()