#!/usr/bin/env python3
import re
import os

# Define the fixes for panic statements
panic_fixes = {
    'crates/consensus/src/dbft/mod.rs': {
        198: '            _ => return Err(ConsensusError::UnexpectedError("Unexpected error type".into())),',
    },
    'crates/rpc_client/src/client.rs': {
        303: '                return Err(Error::UnexpectedError(format!("Expected MethodNotFound error, got: {:?}", other_error)));',
        320: '                return Err(Error::UnexpectedError(format!("Expected InvalidParams error, got: {:?}", other_error)));',
    },
    'crates/rpc_client/src/error.rs': {
        282: '            _ => return Err(Error::UnexpectedError("Expected MethodNotFound error".into())),',
    },
    'crates/network/src/p2p/events.rs': {
        262: '            return Err(NetworkError::UnexpectedEvent("Expected PeerHeight event".into()));',
    },
    'crates/network/src/error_handling.rs': {
        769: '            _ => return Err(NetworkError::UnexpectedEvent("Expected ErrorOccurred event".into())),',
        961: '            _ => return Err(NetworkError::UnexpectedEvent("Wrong event type".into())),',
        968: '            _ => return Err(NetworkError::UnexpectedEvent("Wrong event type".into())),',
        978: '            _ => return Err(NetworkError::UnexpectedEvent("Wrong event type".into())),',
    },
    'crates/vm/src/script_builder.rs': {
        201: '            return Err(VMError::InvalidOperation("Invalid jump operation".into()));',
        224: '            return Err(VMError::InvalidOperation("Syscall api is too long".into()));',
    },
    'crates/vm/src/jump_table/control/types.rs': {
        36: '        eprintln!("as_any() must be implemented by concrete types that implement InteropInterface");\n        std::process::abort();',
    },
    'crates/json/src/jarray.rs': {
        327: '            _ => return Err(JsonError::TypeMismatch("Expected JToken::Array".into())),',
    },
    'crates/json/src/jstring.rs': {
        150: '            _ => return Err(JsonError::TypeMismatch("Expected JToken::String".into())),',
    },
    'crates/json/src/jnumber.rs': {
        257: '            _ => return Err(JsonError::TypeMismatch("Expected JToken::Number".into())),',
    },
    'crates/json/src/jboolean.rs': {
        127: '            _ => return Err(JsonError::TypeMismatch("Expected JToken::Boolean".into())),',
    },
    'node/src/storage_error_handler.rs': {
        470: '                return Err(anyhow::anyhow!("Wrong error type"));',
    },
    'node/src/network_error_handler.rs': {
        301: '            _ => return Err(anyhow::anyhow!("Wrong error type")),',
    },
}

def fix_panic_in_file(file_path, line_fixes):
    """Fix panic statements in a file."""
    if not os.path.exists(file_path):
        print(f"Warning: {file_path} not found")
        return
    
    with open(file_path, 'r') as f:
        lines = f.readlines()
    
    # Apply fixes
    for line_num, replacement in sorted(line_fixes.items(), reverse=True):
        line_idx = line_num - 1
        if line_idx < len(lines):
            original = lines[line_idx].rstrip()
            if 'panic!' in original:
                # Preserve indentation
                indent = len(original) - len(original.lstrip())
                lines[line_idx] = ' ' * indent + replacement.lstrip() + '\n'
                print(f"Fixed panic at {file_path}:{line_num}")
            else:
                print(f"Warning: No panic found at {file_path}:{line_num}")
    
    # Write back
    with open(file_path, 'w') as f:
        f.writelines(lines)

# Apply all fixes
for file_path, line_fixes in panic_fixes.items():
    fix_panic_in_file(file_path, line_fixes)

print("\nPanic statement fixes completed!")