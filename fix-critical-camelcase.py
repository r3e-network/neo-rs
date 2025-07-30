#!/usr/bin/env python3
"""Fix CamelCase variables in critical production code only."""

import os
import re
import glob

def is_camel_case(name):
    """Check if a name is CamelCase (not snake_case)."""
    # Skip if it's all uppercase (constants)
    if name.isupper():
        return False
    # Skip if it starts with uppercase (types/structs)
    if name[0].isupper():
        return False
    # Check for camelCase pattern
    return bool(re.match(r'^[a-z]+[A-Z]', name))

def fix_camel_case_in_file(file_path):
    """Fix CamelCase variables in a file."""
    try:
        # Skip test files
        if any(skip in file_path for skip in ['test', '/tests/', '/examples/', 'bench']):
            return 0
        
        # Only process critical modules
        if not any(critical in file_path for critical in ['consensus', 'ledger', 'network/src/p2p', 'vm/src/execution']):
            return 0
        
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        original_content = content
        changes_made = 0
        
        # Common CamelCase to snake_case conversions
        replacements = [
            # Common patterns in blockchain code
            ('blockIndex', 'block_index'),
            ('blockHeight', 'block_height'),
            ('txHash', 'tx_hash'),
            ('pubKey', 'pub_key'),
            ('privKey', 'priv_key'),
            ('scriptHash', 'script_hash'),
            ('accountState', 'account_state'),
            ('contractState', 'contract_state'),
            ('validatorIndex', 'validator_index'),
            ('viewNumber', 'view_number'),
            ('primaryIndex', 'primary_index'),
            ('consensusContext', 'consensus_context'),
            ('messageHandler', 'message_handler'),
            ('peerManager', 'peer_manager'),
            ('networkConfig', 'network_config'),
            ('storageKey', 'storage_key'),
            ('storageItem', 'storage_item'),
        ]
        
        for old_name, new_name in replacements:
            # Match as whole words only
            pattern = r'\b' + old_name + r'\b'
            content = re.sub(pattern, new_name, content)
            if old_name in original_content:
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
    """Main function."""
    total_fixes = 0
    files_fixed = 0
    
    for pattern in ['crates/**/*.rs', 'node/**/*.rs']:
        for file_path in glob.glob(pattern, recursive=True):
            if os.path.isfile(file_path):
                fixes = fix_camel_case_in_file(file_path)
                if fixes > 0:
                    print(f"Fixed {fixes} CamelCase variables in {file_path}")
                    total_fixes += fixes
                    files_fixed += 1
    
    print(f"\nTotal CamelCase fixes: {total_fixes} in {files_fixed} files")

if __name__ == '__main__':
    main()