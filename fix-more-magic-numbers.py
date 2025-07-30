#!/usr/bin/env python3
"""Fix more magic numbers in production code."""

import os
import re
import glob

def fix_more_magic_numbers(file_path):
    """Replace remaining magic numbers with named constants."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            content = f.read()
        
        original_content = content
        changes_made = 0
        
        # Skip test files
        if any(skip in file_path for skip in ['test', '/tests/', '/examples/', 'bench']):
            return 0
        
        # Common magic numbers and their replacements
        replacements = [
            # Block time related
            (r'\b15\b(?![\d\.])', 'SECONDS_PER_BLOCK'),
            (r'\b15000\b', 'MILLISECONDS_PER_BLOCK'),
            (r'\b15_000\b', 'MILLISECONDS_PER_BLOCK'),
            
            # Size limits
            (r'\b262144\b', 'MAX_BLOCK_SIZE'),
            (r'\b262_144\b', 'MAX_BLOCK_SIZE'),
            (r'\b102400\b', 'MAX_TRANSACTION_SIZE'),
            (r'\b102_400\b', 'MAX_TRANSACTION_SIZE'),
            (r'\b512\b(?![\d\.])', 'MAX_TRANSACTIONS_PER_BLOCK'),
            
            # Network limits
            (r'\b1048576\b', 'MAX_BLOCK_SIZE'),
            (r'\b1_048_576\b', 'MAX_BLOCK_SIZE'),
            
            # Common sizes
            (r'\b65535\b', 'u16::MAX'),
            (r'\b65_535\b', 'u16::MAX'),
            (r'\b4294967295\b', 'u32::MAX'),
            (r'\b4_294_967_295\b', 'u32::MAX'),
            
            # Hash sizes
            (r'\b32\b(?![\d\.])', 'HASH_SIZE'),  # For UInt256
            (r'\b20\b(?![\d\.])', 'ADDRESS_SIZE'),  # For UInt160
            
            # Script limits
            (r'\b1024\b(?![\d\.])', 'MAX_SCRIPT_SIZE'),
            (r'\b65536\b', 'MAX_SCRIPT_LENGTH'),
            (r'\b65_536\b', 'MAX_SCRIPT_LENGTH'),
        ]
        
        # Track which constants need to be imported
        needed_imports = set()
        
        for pattern, replacement in replacements:
            if re.search(pattern, content):
                # Check if this is already using the constant
                if replacement not in content:
                    needed_imports.add(replacement)
                
                # Replace the magic number
                content = re.sub(pattern, replacement, content)
                changes_made += len(re.findall(pattern, original_content))
        
        # Add imports if needed
        if needed_imports and changes_made > 0:
            # Find where to add imports
            import_line = None
            
            # Look for existing use statements
            use_pattern = r'^use\s+'
            for i, line in enumerate(content.splitlines()):
                if re.match(use_pattern, line):
                    import_line = i
            
            if import_line is not None:
                lines = content.splitlines()
                
                # Add config import if needed
                config_constants = {'SECONDS_PER_BLOCK', 'MILLISECONDS_PER_BLOCK', 
                                  'MAX_BLOCK_SIZE', 'MAX_TRANSACTION_SIZE', 
                                  'MAX_TRANSACTIONS_PER_BLOCK', 'HASH_SIZE', 
                                  'ADDRESS_SIZE', 'MAX_SCRIPT_SIZE', 'MAX_SCRIPT_LENGTH'}
                
                needed_config = needed_imports.intersection(config_constants)
                if needed_config:
                    # Check if config is already imported
                    if 'use neo_config::' not in content:
                        import_str = f"use neo_config::{{{', '.join(sorted(needed_config))}}};"
                        lines.insert(import_line + 1, import_str)
                        changes_made += 1
                
                content = '\n'.join(lines)
        
        if content != original_content:
            with open(file_path, 'w', encoding='utf-8') as f:
                f.write(content)
            print(f"Fixed {changes_made} magic numbers in {file_path}")
            return changes_made
        
        return 0
    
    except Exception as e:
        print(f"Error processing {file_path}: {e}")
        return 0

def main():
    """Main function to fix more magic numbers."""
    total_fixes = 0
    
    # First, ensure constants are defined in config
    config_path = '/Users/jinghuiliao/git/r3e/neo-rs/crates/config/src/lib.rs'
    if os.path.exists(config_path):
        with open(config_path, 'r') as f:
            config_content = f.read()
        
        # Add missing constants if needed
        missing_constants = []
        
        if 'pub const HASH_SIZE: usize = 32;' not in config_content:
            missing_constants.append('/// Size of a hash (UInt256) in bytes\npub const HASH_SIZE: usize = 32;')
        
        if 'pub const ADDRESS_SIZE: usize = 20;' not in config_content:
            missing_constants.append('/// Size of an address (UInt160) in bytes\npub const ADDRESS_SIZE: usize = 20;')
        
        if 'pub const MAX_SCRIPT_SIZE: usize = 1024;' not in config_content:
            missing_constants.append('/// Maximum script size in bytes\npub const MAX_SCRIPT_SIZE: usize = 1024;')
        
        if 'pub const MAX_SCRIPT_LENGTH: usize = 65536;' not in config_content:
            missing_constants.append('/// Maximum script length (64KB)\npub const MAX_SCRIPT_LENGTH: usize = 65536;')
        
        if missing_constants:
            # Find where to add constants (after existing constants)
            lines = config_content.splitlines()
            insert_pos = None
            
            for i, line in enumerate(lines):
                if 'pub const' in line:
                    insert_pos = i + 1
            
            if insert_pos:
                for const in missing_constants:
                    lines.insert(insert_pos, const)
                    insert_pos += 1
                
                with open(config_path, 'w') as f:
                    f.write('\n'.join(lines))
                
                print(f"Added {len(missing_constants)} missing constants to config")
    
    # Process all Rust files
    for pattern in ['crates/**/*.rs', 'node/**/*.rs']:
        for file_path in glob.glob(pattern, recursive=True):
            if os.path.isfile(file_path):
                fixes = fix_more_magic_numbers(file_path)
                total_fixes += fixes
    
    print(f"\nTotal magic numbers fixed: {total_fixes}")

if __name__ == '__main__':
    main()