#!/usr/bin/env python3
"""Fix final wildcard imports."""

import os
import re
import glob

def fix_final_wildcards(file_path):
    """Fix final wildcard imports in a file."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        original_content = content
        changes_made = 0
        
        # Skip test files
        if any(skip in file_path for skip in ['test', '/tests/', '/examples/', 'bench']):
            return 0
        
        # Fix specific patterns
        
        # 1. Fix use varlen::*;
        if 'use varlen::*;' in content:
            content = content.replace('use varlen::*;', 'use varlen::{VarInt, VarLen};')
            changes_made += 1
        
        # 2. Fix use super::*; in non-test code
        if 'use super::*;' in content and '#[cfg(test)]' not in content:
            content = content.replace('use super::*;', '')
            changes_made += 1
        
        # 3. Fix use ErrorCategory::*;
        if 'use ErrorCategory::*;' in content:
            content = content.replace('use ErrorCategory::*;', 'use ErrorCategory::{Network, Storage, Consensus, VM, Configuration};')
            changes_made += 1
        
        # 4. Fix std::prelude::v1::*
        if 'use std::prelude::v1::*;' in content:
            # This is actually fine - it's the standard prelude
            # But we can remove it as it's automatically imported
            content = content.replace('use std::prelude::v1::*;', '')
            changes_made += 1
        
        # 5. For pub use module::*, these are often intentional re-exports
        # But we can be more specific in some cases
        if file_path.endswith('transaction/mod.rs'):
            # For transaction module, be more specific
            replacements = [
                ('pub use attributes::*;', 'pub use attributes::{TransactionAttribute, TransactionAttributeType, OracleResponse, OracleResponseCode, NotValidBefore, Conflicts};'),
                ('pub use blockchain::*;', 'pub use blockchain::{TransactionContext, TransactionVerification};'),
                ('pub use core::*;', 'pub use core::{Transaction, TransactionType, Witness};'),
                ('pub use vm::*;', 'pub use vm::{TransactionVM, VMTransaction};'),
            ]
            
            for old, new in replacements:
                if old in content:
                    content = content.replace(old, new)
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
    """Main function to fix final wildcards."""
    total_fixes = 0
    files_fixed = 0
    
    # Target specific files
    target_files = [
        'crates/core/src/transaction/mod.rs',
        'crates/network/src/messages/commands.rs',
        'crates/vm/src/lib.rs',
        'node/src/error_handler.rs',
    ]
    
    for file_path in target_files:
        if os.path.exists(file_path):
            fixes = fix_final_wildcards(file_path)
            if fixes > 0:
                print(f"Fixed {fixes} wildcard imports in {file_path}")
                total_fixes += fixes
                files_fixed += 1
    
    print(f"\nTotal wildcard imports fixed: {total_fixes} in {files_fixed} files")

if __name__ == '__main__':
    main()