#!/usr/bin/env python3
"""Aggressively fix unwraps in VM module."""

import os
import re

def fix_vm_unwraps():
    """Fix unwraps in VM module files."""
    
    # Target the files with most unwraps
    target_files = [
        'crates/vm/src/jump_table/bitwise.rs',
        'crates/vm/src/evaluation_stack.rs',
        'crates/vm/src/stack_item/struct_item.rs',
        'crates/vm/src/stack_item/map.rs',
        'crates/vm/src/stack_item/array.rs',
        'crates/vm/src/stack_item/stack_item.rs',
        'crates/vm/src/execution_context.rs',
        'crates/vm/src/execution_engine.rs',
        'crates/vm/src/instruction.rs',
        'crates/vm/src/script.rs',
        'crates/vm/src/jump_table/crypto.rs'
    ]
    
    total_fixes = 0
    
    for file_path in target_files:
        if not os.path.exists(file_path):
            continue
        
        try:
            with open(file_path, 'r', encoding='utf-8') as f:
                content = f.read()
            
            original_content = content
            fixes = 0
            
            # Common VM patterns
            replacements = [
                # pop().unwrap() -> pop()?
                (r'\.pop\(\)\.unwrap\(\)', '.pop()?'),
                
                # peek().unwrap() -> peek()?
                (r'\.peek\(\)\.unwrap\(\)', '.peek()?'),
                
                # get().unwrap() -> get().ok_or(VmError)?
                (r'\.get\(([^)]+)\)\.unwrap\(\)', r'.get(\1).ok_or(VmError::InvalidOperation)?'),
                
                # to_integer().unwrap() -> to_integer()?
                (r'\.to_integer\(\)\.unwrap\(\)', '.to_integer()?'),
                
                # to_boolean().unwrap() -> to_boolean()?
                (r'\.to_boolean\(\)\.unwrap\(\)', '.to_boolean()?'),
                
                # to_array().unwrap() -> to_array()?
                (r'\.to_array\(\)\.unwrap\(\)', '.to_array()?'),
                
                # downcast_ref().unwrap() -> downcast_ref().ok_or(VmError)?
                (r'\.downcast_ref::<([^>]+)>\(\)\.unwrap\(\)', r'.downcast_ref::<\1>().ok_or(VmError::InvalidType)?'),
                
                # borrow().unwrap() -> borrow().map_err(|_| VmError)?
                (r'\.borrow\(\)\.unwrap\(\)', '.borrow().map_err(|_| VmError::InvalidOperation)?'),
                
                # borrow_mut().unwrap() -> borrow_mut().map_err(|_| VmError)?
                (r'\.borrow_mut\(\)\.unwrap\(\)', '.borrow_mut().map_err(|_| VmError::InvalidOperation)?'),
                
                # Lock patterns specific to VM
                (r'\.lock\(\)\.unwrap\(\)', '.lock().map_err(|_| VmError::LockError)?'),
                (r'\.write\(\)\.unwrap\(\)', '.write().map_err(|_| VmError::LockError)?'),
                (r'\.read\(\)\.unwrap\(\)', '.read().map_err(|_| VmError::LockError)?'),
            ]
            
            for pattern, replacement in replacements:
                new_content = re.sub(pattern, replacement, content)
                if new_content != content:
                    fixes += content.count('.unwrap()') - new_content.count('.unwrap()')
                    content = new_content
            
            # Handle specific patterns that need context
            lines = content.splitlines()
            new_lines = []
            
            for i, line in enumerate(lines):
                # Check if we're in a function that returns Result
                in_result_fn = False
                if i > 0:
                    for j in range(max(0, i-20), i):
                        if 'fn ' in lines[j] and '-> Result' in lines[j]:
                            in_result_fn = True
                            break
                        elif 'fn ' in lines[j] and '-> VmResult' in lines[j]:
                            in_result_fn = True
                            break
                
                # If in Result function, convert remaining unwraps to ?
                if in_result_fn and '.unwrap()' in line and not line.strip().startswith('//'):
                    # Simple unwrap at end of line
                    if line.rstrip().endswith('.unwrap()'):
                        new_line = line.replace('.unwrap()', '?')
                        fixes += 1
                    # unwrap in middle of expression  
                    elif '.unwrap().' in line:
                        new_line = line.replace('.unwrap().', '?.', 1)
                        fixes += 1
                    else:
                        new_line = line
                else:
                    new_line = line
                
                new_lines.append(new_line)
            
            if fixes > 0:
                content = '\n'.join(new_lines)
                
                # Ensure proper imports
                if '?' in content and 'Result' in content:
                    # Make sure VmError is imported
                    if 'use crate::VmError;' not in content and 'VmError' in content:
                        # Add import after other use statements
                        import_added = False
                        final_lines = content.splitlines()
                        for i, line in enumerate(final_lines):
                            if line.startswith('use ') and not import_added:
                                continue
                            elif not line.startswith('use ') and i > 0 and not import_added:
                                final_lines.insert(i, 'use crate::VmError;')
                                import_added = True
                                break
                        content = '\n'.join(final_lines)
                
                with open(file_path, 'w', encoding='utf-8') as f:
                    f.write(content)
                print(f"Fixed {fixes} unwraps in {file_path}")
                total_fixes += fixes
                
        except Exception as e:
            print(f"Error processing {file_path}: {e}")
    
    return total_fixes

if __name__ == '__main__':
    total = fix_vm_unwraps()
    print(f"\nTotal unwraps fixed: {total}")