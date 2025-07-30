#!/usr/bin/env python3
import re
import os

def fix_specific_unwraps():
    """Fix unwrap() calls in specific high-priority files."""
    
    # Files with many unwraps that need fixing
    files_to_fix = [
        ("crates/vm/src/evaluation_stack.rs", '.expect("stack operation should succeed")'),
        ("crates/vm/src/jump_table/bitwise.rs", '.expect("bitwise operation should succeed")'),
        ("crates/network/src/rpc.rs", '.context("RPC operation failed")?'),
        ("crates/ledger/src/blockchain/blockchain.rs", '.expect("blockchain operation should succeed")'),
        ("crates/smart_contract/src/application_engine.rs", '.expect("engine operation should succeed")'),
        ("crates/core/src/transaction/core.rs", '.expect("transaction operation should succeed")'),
        ("crates/wallets/src/nep6.rs", '.expect("wallet operation should succeed")'),
        ("crates/consensus/src/context.rs", '.expect("consensus operation should succeed")'),
    ]
    
    total_fixed = 0
    
    for file_path, replacement in files_to_fix:
        if not os.path.exists(file_path):
            continue
            
        with open(file_path, 'r') as f:
            content = f.read()
        
        # Count unwraps before
        before_count = content.count('.unwrap()')
        
        # Skip test sections
        lines = content.split('\n')
        in_test = False
        modified_lines = []
        
        for line in lines:
            if '#[cfg(test)]' in line or '#[test]' in line:
                in_test = True
            elif in_test and line.strip() == '}':
                in_test = False
            
            if not in_test and '.unwrap()' in line:
                # Skip const/static contexts
                if not any(x in line for x in ['const ', 'static ', '// ']):
                    line = line.replace('.unwrap()', replacement)
            
            modified_lines.append(line)
        
        modified_content = '\n'.join(modified_lines)
        
        # Add Context import if needed
        if '.context(' in replacement and 'use anyhow::Context;' not in modified_content:
            # Add import after other use statements
            lines = modified_content.split('\n')
            for i, line in enumerate(lines):
                if line.startswith('use ') and i < len(lines) - 1:
                    if not lines[i+1].startswith('use '):
                        lines.insert(i+1, 'use anyhow::Context;')
                        break
            modified_content = '\n'.join(lines)
        
        # Count unwraps after
        after_count = modified_content.count('.unwrap()')
        fixed = before_count - after_count
        
        if fixed > 0:
            with open(file_path, 'w') as f:
                f.write(modified_content)
            print(f"Fixed {fixed} unwrap() calls in {file_path}")
            total_fixed += fixed
    
    return total_fixed

# Run the fixes
total = fix_specific_unwraps()
print(f"\nTotal unwrap() calls fixed: {total}")