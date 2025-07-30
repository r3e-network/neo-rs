#!/usr/bin/env python3
"""Fix last remaining wildcard imports."""

import os
import re

def fix_last_wildcards():
    """Fix the last few wildcard imports."""
    fixes = []
    
    # Fix rpc_client/src/lib.rs
    file_path = 'crates/rpc_client/src/lib.rs'
    if os.path.exists(file_path):
        with open(file_path, 'r') as f:
            content = f.read()
        
        if 'pub use models::*;' in content:
            # Find what's in models module
            new_content = content.replace(
                'pub use models::*;',
                'pub use models::{RpcRequest, RpcResponse, RpcError, GetBlockParams, GetTransactionParams, InvokeContractParams, SendRawTransactionParams};'
            )
            
            with open(file_path, 'w') as f:
                f.write(new_content)
            fixes.append(file_path)
            print(f"Fixed wildcard import in {file_path}")
    
    # Fix vm/src/jump_table/control/mod.rs
    file_path = 'crates/vm/src/jump_table/control/mod.rs'
    if os.path.exists(file_path):
        with open(file_path, 'r') as f:
            content = f.read()
        
        if 'pub use types::*;' in content:
            new_content = content.replace(
                'pub use types::*;',
                'pub use types::{ControlInstruction, ControlFlowHandler, ControlFlowResult};'
            )
            
            with open(file_path, 'w') as f:
                f.write(new_content)
            fixes.append(file_path)
            print(f"Fixed wildcard import in {file_path}")
    
    # Fix node/src/vm_integration.rs
    file_path = 'node/src/vm_integration.rs'
    if os.path.exists(file_path):
        with open(file_path, 'r') as f:
            content = f.read()
        
        if 'use super::stack_item_helpers::*;' in content:
            new_content = content.replace(
                'use super::stack_item_helpers::*;',
                'use super::stack_item_helpers::{to_stack_item, from_stack_item};'
            )
            
            with open(file_path, 'w') as f:
                f.write(new_content)
            fixes.append(file_path)
            print(f"Fixed wildcard import in {file_path}")
    
    return fixes

if __name__ == '__main__':
    fixes = fix_last_wildcards()
    print(f"\nFixed {len(fixes)} files with wildcard imports")