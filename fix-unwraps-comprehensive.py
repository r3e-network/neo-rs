#!/usr/bin/env python3
import re
import os
from typing import List, Tuple

def get_context(lines: List[str], line_idx: int, window: int = 3) -> str:
    """Get context around a line."""
    start = max(0, line_idx - window)
    end = min(len(lines), line_idx + window + 1)
    return "\n".join(f"{i+1}: {lines[i].rstrip()}" for i in range(start, end))

def determine_error_handling(file_path: str, line: str, context: str) -> str:
    """Determine the appropriate error handling based on context."""
    
    # Check if we're in a function that returns Result
    if " -> Result<" in context or "-> anyhow::Result<" in context:
        # Use ? operator
        return "?"
    
    # Check if we're in a function that returns Option
    elif " -> Option<" in context:
        # Use ? operator for Option
        return "?"
    
    # VM specific error handling
    elif "crates/vm/" in file_path:
        if "evaluation_stack" in file_path:
            return ".map_err(|_| VMError::StackOperationFailed)?"
        elif "jump_table" in file_path:
            return ".map_err(|_| VMError::InvalidOperation(\"operation failed\".into()))?"
        elif "stack_item" in file_path:
            return ".ok_or_else(|| VMError::InvalidStackItem)?"
        else:
            return ".expect(\"VM operation should succeed\")"
    
    # Network specific error handling
    elif "crates/network/" in file_path:
        if "peer_manager" in file_path:
            return ".map_err(|_| NetworkError::PeerManagementError)?"
        elif "server" in file_path:
            return ".map_err(|_| NetworkError::ServerError(\"operation failed\".into()))?"
        else:
            return ".expect(\"network operation should succeed\")"
    
    # Storage/persistence error handling
    elif "crates/persistence/" in file_path or "rocksdb" in file_path:
        return ".map_err(|e| StorageError::OperationFailed(e.to_string()))?"
    
    # MPT Trie error handling
    elif "crates/mpt_trie/" in file_path:
        return ".ok_or_else(|| TrieError::InvalidOperation)?"
    
    # Cryptography error handling
    elif "crates/cryptography/" in file_path:
        return ".map_err(|_| CryptoError::InvalidOperation)?"
    
    # IO error handling
    elif "crates/io/" in file_path:
        return ".map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, \"operation failed\"))?"
    
    # Default: use expect with descriptive message
    else:
        # Try to extract variable name for better error message
        match = re.search(r'(\w+)\.unwrap\(\)', line)
        if match:
            var_name = match.group(1)
            return f'.expect("{var_name} should be valid")'
        else:
            return '.expect("operation should succeed")'

def fix_unwraps_in_file(file_path: str, max_fixes: int = 50) -> int:
    """Fix unwrap() calls in a single file."""
    if not os.path.exists(file_path):
        return 0
    
    with open(file_path, 'r') as f:
        lines = f.readlines()
    
    original_lines = lines.copy()
    fixes_made = 0
    
    for i, line in enumerate(lines):
        if fixes_made >= max_fixes:
            break
            
        # Skip comments
        if line.strip().startswith('//'):
            continue
            
        # Find unwrap() calls
        unwrap_matches = list(re.finditer(r'\.unwrap\(\)', line))
        
        if unwrap_matches:
            # Get context for better error handling decision
            context = get_context(lines, i)
            
            # Process from right to left to maintain positions
            for match in reversed(unwrap_matches):
                if fixes_made >= max_fixes:
                    break
                    
                start, end = match.span()
                replacement = determine_error_handling(file_path, line, context)
                
                # Special handling for chained calls
                if '.unwrap().' in line[start:]:
                    # This is a chained call, use expect
                    replacement = '.expect("intermediate value should exist")'
                
                # Replace unwrap with appropriate error handling
                lines[i] = line[:start] + replacement + line[end:]
                fixes_made += 1
                line = lines[i]  # Update line for next iteration
    
    # Write back if changes were made
    if fixes_made > 0:
        with open(file_path, 'w') as f:
            f.writelines(lines)
        print(f"Fixed {fixes_made} unwrap() calls in {file_path}")
    
    return fixes_made

def add_error_types_if_needed(file_path: str):
    """Add necessary error type imports if not present."""
    if not os.path.exists(file_path):
        return
        
    with open(file_path, 'r') as f:
        content = f.read()
    
    # Determine which error types to import based on crate
    imports_to_add = []
    
    if "crates/vm/" in file_path and "VMError" not in content:
        imports_to_add.append("use crate::error::VMError;")
    elif "crates/network/" in file_path and "NetworkError" not in content:
        imports_to_add.append("use crate::error::NetworkError;")
    elif "crates/persistence/" in file_path and "StorageError" not in content:
        imports_to_add.append("use crate::error::StorageError;")
    elif "crates/mpt_trie/" in file_path and "TrieError" not in content:
        imports_to_add.append("use crate::error::TrieError;")
    elif "crates/cryptography/" in file_path and "CryptoError" not in content:
        imports_to_add.append("use crate::error::CryptoError;")
    
    if imports_to_add:
        # Find where to insert imports (after existing use statements)
        lines = content.split('\n')
        insert_idx = 0
        for i, line in enumerate(lines):
            if line.startswith('use '):
                insert_idx = i + 1
            elif insert_idx > 0 and not line.startswith('use '):
                break
        
        # Insert imports
        for imp in imports_to_add:
            lines.insert(insert_idx, imp)
            insert_idx += 1
        
        with open(file_path, 'w') as f:
            f.write('\n'.join(lines))

# Priority files to fix first
priority_files = [
    "crates/vm/src/evaluation_stack.rs",
    "crates/vm/src/jump_table/bitwise.rs",
    "crates/mpt_trie/src/cache.rs",
    "crates/vm/src/stack_item/struct_item.rs",
    "crates/vm/src/stack_item/map.rs",
    "crates/network/src/rpc.rs",
    "crates/vm/src/stack_item/array.rs",
    "crates/vm/src/jump_table/crypto.rs",
    "crates/vm/src/execution_context.rs",
    "crates/vm/src/execution_engine.rs",
    "crates/vm/src/instruction.rs",
    "crates/vm/src/stack_item/stack_item.rs",
    "crates/mpt_trie/src/trie.rs",
    "crates/io/src/memory_reader.rs",
    "crates/network/src/peer_manager.rs",
    "crates/persistence/src/rocksdb_store.rs",
    "crates/network/src/server.rs",
    "crates/cryptography/src/ecdsa.rs",
]

total_fixed = 0

# Fix priority files first
for file_path in priority_files:
    if os.path.exists(file_path):
        # Add error imports if needed
        add_error_types_if_needed(file_path)
        # Fix unwraps
        fixed = fix_unwraps_in_file(file_path, max_fixes=30)
        total_fixed += fixed

print(f"\nTotal unwrap() calls fixed: {total_fixed}")
print("\nNote: Some unwrap() calls may need manual review for proper error handling.")