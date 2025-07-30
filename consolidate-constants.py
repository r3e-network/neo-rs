#!/usr/bin/env python3
"""Consolidate duplicate constants across the Neo-RS codebase."""

import os
import re
from collections import defaultdict

def consolidate_constants():
    """Move duplicate constants to common locations."""
    
    # These constants should be in config/src/lib.rs
    network_constants = {
        'DEFAULT_NEO_PORT': '"10333"',
        'DEFAULT_RPC_PORT': '"10332"', 
        'DEFAULT_TESTNET_PORT': '"20333"',
        'DEFAULT_TESTNET_RPC_PORT': '"20332"',
    }
    
    # These constants should be in core/src/constants.rs
    blockchain_constants = {
        'MAX_STORAGE_KEY_SIZE': '64',
        'MAX_STORAGE_VALUE_SIZE': 'u16::MAX',
        'MAX_MESSAGE_SIZE': '16 * MAX_SCRIPT_SIZE * MAX_SCRIPT_SIZE',
        'MAX_RETRY_ATTEMPTS': '3',
    }
    
    files_fixed = 0
    
    # Remove duplicate network constants and add imports
    for root, dirs, files in os.walk('crates'):
        dirs[:] = [d for d in dirs if d not in ['tests', 'test', 'examples', 'benches', '.git', 'target']]
        
        for file in files:
            if file.endswith('.rs'):
                filepath = os.path.join(root, file)
                # Skip the config file itself
                if 'config/src/lib.rs' in filepath:
                    continue
                    
                try:
                    with open(filepath, 'r') as f:
                        content = f.read()
                    
                    original_content = content
                    modified = False
                    
                    # Check for duplicate network constants
                    for const_name, const_value in network_constants.items():
                        pattern = rf'^\s*(?:pub\s+)?const\s+{const_name}:\s*[^=]+\s*=\s*{re.escape(const_value)};'
                        if re.search(pattern, content, re.MULTILINE):
                            # Remove the constant definition
                            content = re.sub(pattern + r'\s*\n', '', content, flags=re.MULTILINE)
                            
                            # Add import if not present
                            if f'use neo_config::{const_name}' not in content and 'use neo_config::*' not in content:
                                # Find where to insert the import
                                import_match = re.search(r'^use\s+', content, re.MULTILINE)
                                if import_match:
                                    insert_pos = import_match.start()
                                    content = content[:insert_pos] + f'use neo_config::{const_name};\n' + content[insert_pos:]
                                else:
                                    # Add at the beginning after any doc comments
                                    content = re.sub(r'^((?://.*\n)*)', r'\1use neo_config::' + const_name + ';\n\n', content)
                            
                            modified = True
                            print(f"Removed duplicate {const_name} from {filepath}")
                    
                    # Check for duplicate blockchain constants
                    for const_name, const_value in blockchain_constants.items():
                        pattern = rf'^\s*(?:pub\s+)?const\s+{const_name}:\s*[^=]+\s*=\s*{re.escape(const_value)};'
                        if re.search(pattern, content, re.MULTILINE):
                            # Remove the constant definition
                            content = re.sub(pattern + r'\s*\n', '', content, flags=re.MULTILINE)
                            
                            # Add import if not present
                            if f'use neo_core::constants::{const_name}' not in content:
                                # Find where to insert the import
                                import_match = re.search(r'^use\s+', content, re.MULTILINE)
                                if import_match:
                                    insert_pos = import_match.start()
                                    content = content[:insert_pos] + f'use neo_core::constants::{const_name};\n' + content[insert_pos:]
                                else:
                                    content = re.sub(r'^((?://.*\n)*)', r'\1use neo_core::constants::' + const_name + ';\n\n', content)
                            
                            modified = True
                            print(f"Removed duplicate {const_name} from {filepath}")
                    
                    if modified:
                        with open(filepath, 'w') as f:
                            f.write(content)
                        files_fixed += 1
                        
                except Exception as e:
                    print(f"Error processing {filepath}: {e}")
    
    # Ensure constants exist in their canonical locations
    ensure_constants_exist()
    
    print(f"\nFixed {files_fixed} files")
    return files_fixed

def ensure_constants_exist():
    """Ensure all constants exist in their canonical locations."""
    
    # Check config/src/lib.rs
    config_file = 'crates/config/src/lib.rs'
    if os.path.exists(config_file):
        with open(config_file, 'r') as f:
            content = f.read()
        
        additions = []
        
        # Check for network constants
        if 'DEFAULT_NEO_PORT' not in content:
            additions.append('\n/// Default Neo network ports')
            additions.append('pub const DEFAULT_NEO_PORT: &str = "10333";')
            additions.append('pub const DEFAULT_RPC_PORT: &str = "10332";')
            additions.append('pub const DEFAULT_TESTNET_PORT: &str = "20333";')
            additions.append('pub const DEFAULT_TESTNET_RPC_PORT: &str = "20332";')
        
        if additions:
            # Find a good place to insert (after other constants)
            const_match = re.search(r'(pub const[^;]+;\s*\n)', content)
            if const_match:
                insert_pos = const_match.end()
                content = content[:insert_pos] + '\n'.join(additions) + '\n' + content[insert_pos:]
            else:
                # Add after module doc comment
                content = content + '\n' + '\n'.join(additions) + '\n'
            
            with open(config_file, 'w') as f:
                f.write(content)
            print(f"Added network constants to {config_file}")
    
    # Check core/src/constants.rs
    constants_file = 'crates/core/src/constants.rs'
    if os.path.exists(constants_file):
        with open(constants_file, 'r') as f:
            content = f.read()
        
        additions = []
        
        # Check for storage constants
        if 'MAX_STORAGE_KEY_SIZE' not in content:
            additions.append('\n/// Storage limits')
            additions.append('pub const MAX_STORAGE_KEY_SIZE: usize = 64;')
            additions.append('pub const MAX_STORAGE_VALUE_SIZE: usize = u16::MAX as usize;')
        
        if 'MAX_RETRY_ATTEMPTS' not in content:
            additions.append('\n/// Network retry configuration')
            additions.append('pub const MAX_RETRY_ATTEMPTS: u32 = 3;')
        
        if additions:
            content = content.rstrip() + '\n' + '\n'.join(additions) + '\n'
            
            with open(constants_file, 'w') as f:
                f.write(content)
            print(f"Added blockchain constants to {constants_file}")

if __name__ == '__main__':
    print("=== Consolidating Duplicate Constants ===")
    consolidate_constants()
    print("\nConstant consolidation complete!")
    print("Run ./consistency-check-v5.sh to verify the improvements")