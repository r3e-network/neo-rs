#!/usr/bin/env python3
"""Fix remaining wildcard imports."""

import os
import re
import glob

def fix_remaining_wildcards(file_path):
    """Fix remaining wildcard imports in a file."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        original_content = content
        changes_made = 0
        
        # Skip test files
        if any(skip in file_path for skip in ['test', '/tests/', '/examples/', 'bench']):
            return 0
        
        # Find and fix remaining use super::*
        if 'use super::*;' in content:
            # Different replacements based on module context
            if 'error' in file_path.lower():
                content = content.replace('use super::*;', 'use super::{Error, Result};')
            elif 'mod.rs' in file_path:
                # For mod.rs files, import common items from parent
                content = content.replace('use super::*;', 'use super::{Error, Result};')
            else:
                # Generic replacement
                content = content.replace('use super::*;', '')
            changes_made += content.count('use super::') - original_content.count('use super::*;')
        
        # Fix other wildcard patterns
        wildcard_patterns = [
            (r'use self::\*;', ''),
            (r'use crate::\*;', ''),
            (r'use crate::(\w+)::\*;', r'use crate::\1::{Error, Result};'),
        ]
        
        for pattern, replacement in wildcard_patterns:
            if re.search(pattern, content):
                content = re.sub(pattern, replacement, content)
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
    """Main function to fix remaining wildcards."""
    total_fixes = 0
    files_fixed = 0
    
    # Process all Rust files
    for pattern in ['crates/**/*.rs', 'node/**/*.rs']:
        for file_path in glob.glob(pattern, recursive=True):
            if os.path.isfile(file_path):
                fixes = fix_remaining_wildcards(file_path)
                if fixes > 0:
                    print(f"Fixed {fixes} wildcard imports in {file_path}")
                    total_fixes += fixes
                    files_fixed += 1
    
    print(f"\nTotal wildcard imports fixed: {total_fixes} in {files_fixed} files")
    
    # Count remaining wildcards
    remaining = 0
    for pattern in ['crates/**/*.rs', 'node/**/*.rs']:
        for file_path in glob.glob(pattern, recursive=True):
            if os.path.isfile(file_path) and 'test' not in file_path:
                with open(file_path, 'r') as f:
                    content = f.read()
                    remaining += content.count('::*;')
    
    print(f"Remaining wildcard imports: {remaining}")

if __name__ == '__main__':
    main()