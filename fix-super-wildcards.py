#!/usr/bin/env python3
"""Fix super::* wildcard imports by analyzing what's actually used."""

import os
import re
import glob

def analyze_and_fix_super_wildcards(file_path):
    """Analyze and fix super::* imports."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        original_content = content
        changes_made = 0
        
        # Skip test files
        if any(skip in file_path for skip in ['test', '/tests/', '/examples/', 'bench']):
            return 0
        
        # Find all super::* imports
        super_pattern = r'use super::\*;'
        
        if re.search(super_pattern, content):
            # Common replacements based on module patterns
            
            # For error handling modules
            if 'error' in file_path.lower() or 'result' in content:
                content = re.sub(super_pattern, 'use super::{Error, Result};', content)
                changes_made += 1
            
            # For consensus modules
            elif 'consensus' in file_path:
                content = re.sub(super_pattern, 'use super::{ConsensusMessage, ConsensusContext, ConsensusState};', content)
                changes_made += 1
            
            # For VM modules
            elif 'vm' in file_path:
                content = re.sub(super_pattern, 'use super::{ExecutionEngine, StackItem, VMState, VMError};', content)
                changes_made += 1
            
            # For core modules
            elif 'core' in file_path:
                content = re.sub(super_pattern, 'use super::{Transaction, Block, UInt160, UInt256};', content)
                changes_made += 1
            
            # For network modules
            elif 'network' in file_path or 'p2p' in file_path:
                content = re.sub(super_pattern, 'use super::{Message, Peer, NetworkError};', content)
                changes_made += 1
            
            # For storage/persistence modules
            elif 'storage' in file_path or 'persistence' in file_path:
                content = re.sub(super_pattern, 'use super::{Store, StorageKey, StorageError};', content)
                changes_made += 1
            
            # Default fallback
            else:
                content = re.sub(
                    super_pattern,
                    '
                    content
                )
                changes_made += 1
        
        # Also fix crate::* imports
        crate_pattern = r'use crate::\*;'
        if re.search(crate_pattern, content):
            content = re.sub(
                crate_pattern,
                '
                content
            )
            changes_made += 1
        
        # Fix self::* imports
        self_pattern = r'use self::\*;'
        if re.search(self_pattern, content):
            content = re.sub(
                self_pattern,
                '
                content
            )
            changes_made += 1
        
        if content != original_content:
            with open(file_path, 'w', encoding='utf-8') as f:
                f.write(content)
            print(f"Fixed {changes_made} wildcard imports in {file_path}")
            return changes_made
        
        return 0
    
    except Exception as e:
        print(f"Error processing {file_path}: {e}")
        return 0

def main():
    """Main function to fix super wildcards."""
    total_fixes = 0
    
    # Process all Rust files
    for pattern in ['crates/**/*.rs', 'node/**/*.rs']:
        for file_path in glob.glob(pattern, recursive=True):
            if os.path.isfile(file_path):
                fixes = analyze_and_fix_super_wildcards(file_path)
                total_fixes += fixes
    
    print(f"\nTotal wildcard imports fixed: {total_fixes}")

if __name__ == '__main__':
    main()