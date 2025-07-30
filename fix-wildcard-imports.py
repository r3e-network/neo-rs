#\!/usr/bin/env python3
"""Fix wildcard imports in production code."""

import os
import re
import glob

def fix_wildcard_imports(file_path):
    """Replace wildcard imports with specific imports."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        original_content = content
        changes_made = 0
        
        # Skip test files
        if any(skip in file_path for skip in ['test', '/tests/', '/examples/', 'bench']):
            return 0
        
        # Common wildcard import patterns and their replacements
        import_replacements = [
            # Standard library
            (r'use std::collections::\*;', 'use std::collections::{HashMap, HashSet, BTreeMap, BTreeSet};'),
            (r'use std::io::\*;', 'use std::io::{Read, Write, Error as IoError, Result as IoResult};'),
            (r'use std::fmt::\*;', 'use std::fmt::{self, Display, Debug, Formatter, Result as FmtResult};'),
            (r'use std::sync::\*;', 'use std::sync::{Arc, Mutex, RwLock, Condvar};'),
            (r'use std::convert::\*;', 'use std::convert::{TryFrom, TryInto, From, Into};'),
            (r'use std::ops::\*;', 'use std::ops::{Deref, DerefMut, Add, Sub, Mul, Div};'),
            
            # Async/futures
            (r'use futures::\*;', 'use futures::{future, stream, Future, Stream};'),
            (r'use tokio::sync::\*;', 'use tokio::sync::{mpsc, oneshot, Mutex, RwLock};'),
            (r'use tokio::io::\*;', 'use tokio::io::{AsyncRead, AsyncWrite, AsyncReadExt, AsyncWriteExt};'),
            
            # Serde
            (r'use serde::\*;', 'use serde::{Serialize, Deserialize};'),
            (r'use serde_json::\*;', 'use serde_json::{json, Value, to_string, from_str};'),
            
            # Neo specific
            (r'use neo_core::\*;', 'use neo_core::{Transaction, Block, UInt160, UInt256};'),
            (r'use neo_vm::\*;', 'use neo_vm::{ExecutionEngine, StackItem, VMState};'),
            (r'use neo_cryptography::\*;', 'use neo_cryptography::{hash256, hash160, ECPoint, ECDsa};'),
        ]
        
        for pattern, replacement in import_replacements:
            if re.search(pattern, content):
                content = re.sub(pattern, replacement, content)
                changes_made += 1
        
        # Handle more complex wildcard imports with modules
        # Pattern: use module::submodule::*;
        complex_pattern = r'use\s+([a-zA-Z0-9_]+(?:::[a-zA-Z0-9_]+)*)::\*;'
        
        # For each wildcard import found, try to determine what's actually used
        for match in re.finditer(complex_pattern, content):
            module_path = match.group(1)
            wildcard_import = match.group(0)
            
            # Skip if already processed
            if wildcard_import not in content:
                continue
            
            # Common module-specific replacements
            if 'neo_io' in module_path:
                replacement = f'use {module_path}::{{BinaryReader, BinaryWriter, Serializable}};'
            elif 'neo_config' in module_path:
                replacement = f'use {module_path}::{{SECONDS_PER_BLOCK, MAX_BLOCK_SIZE, MAX_TRANSACTION_SIZE}};'
            elif 'anyhow' in module_path:
                replacement = f'use {module_path}::{{Result, Error, anyhow, bail}};'
            elif 'log' in module_path:
                replacement = f'use {module_path}::{{info, warn, error, debug, trace}};'
            elif 'num_traits' in module_path:
                replacement = f'use {module_path}::{{Zero, One, ToPrimitive, FromPrimitive}};'
            elif 'num_bigint' in module_path:
                replacement = f'use {module_path}::{{BigInt, BigUint, Sign}};'
            else:
                replacement = f'
            
            content = content.replace(wildcard_import, replacement, 1)
            changes_made += 1
        
        # Fix use super::* and use crate::*
        simple_wildcards = [
            (r'use super::\*;', 'use super::{Error, Result};'),
            (r'use crate::\*;', '
        ]
        
        for pattern, replacement in simple_wildcards:
            if re.search(pattern, content):
                content = re.sub(pattern, replacement, content)
                changes_made += 1
        
        if content \!= original_content:
            with open(file_path, 'w', encoding='utf-8') as f:
                f.write(content)
            print(f"Fixed {changes_made} wildcard imports in {file_path}")
            return changes_made
        
        return 0
    
    except Exception as e:
        print(f"Error processing {file_path}: {e}")
        return 0

def main():
    """Main function to fix wildcard imports."""
    total_fixes = 0
    
    # Process all Rust files
    for pattern in ['crates/**/*.rs', 'node/**/*.rs']:
        for file_path in glob.glob(pattern, recursive=True):
            if os.path.isfile(file_path):
                fixes = fix_wildcard_imports(file_path)
                total_fixes += fixes
    
    print(f"\\nTotal wildcard imports fixed: {total_fixes}")

if __name__ == '__main__':
    main()
EOF < /dev/null