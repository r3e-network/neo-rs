#!/usr/bin/env python3
import re
import os

# Define constants for default addresses
DEFAULT_ADDRESSES = {
    "UNKNOWN_PEER_ADDR": '"0.0.0.0:0"',
    "DEFAULT_MAINNET_PORT": '10333',
    "DEFAULT_TESTNET_PORT": '20333',
    "DEFAULT_PRIVNET_PORT": '30333',
    "DEFAULT_RPC_PORT": '10332',
    "DEFAULT_WS_PORT": '10334',
    "LOCALHOST": '"127.0.0.1"',
}

def add_constants_to_file(file_path, constants_needed):
    """Add constant definitions to a file."""
    if not os.path.exists(file_path):
        return
        
    with open(file_path, 'r') as f:
        lines = f.readlines()
    
    # Find where to insert constants (after imports)
    insert_idx = 0
    for i, line in enumerate(lines):
        if line.startswith('use ') or line.startswith('pub use'):
            insert_idx = i + 1
        elif insert_idx > 0 and not line.startswith('use ') and not line.strip() == '':
            break
    
    # Add constants
    constants_added = []
    for const_name, const_value in constants_needed.items():
        const_line = f'const {const_name}: &str = {const_value};\n'
        if const_line not in ''.join(lines):
            lines.insert(insert_idx, const_line)
            constants_added.append(const_name)
            insert_idx += 1
    
    if constants_added:
        # Add blank line after constants
        lines.insert(insert_idx, '\n')
        
        with open(file_path, 'w') as f:
            f.writelines(lines)
        
        print(f"Added constants to {file_path}: {', '.join(constants_added)}")

def fix_hardcoded_ips_in_file(file_path):
    """Fix hardcoded IP addresses in a file."""
    if not os.path.exists(file_path):
        return 0
        
    with open(file_path, 'r') as f:
        content = f.read()
        lines = content.split('\n')
    
    fixed = 0
    constants_needed = {}
    
    for i, line in enumerate(lines):
        # Skip comments
        if line.strip().startswith('//'):
            continue
            
        # Replace specific IP patterns
        replacements = [
            (r'"0\.0\.0\.0:0"', 'UNKNOWN_PEER_ADDR', {"UNKNOWN_PEER_ADDR": '"0.0.0.0:0"'}),
            (r'"127\.0\.0\.1:10333"', 'format!("{}:{}", LOCALHOST, DEFAULT_MAINNET_PORT)', {"LOCALHOST": '"127.0.0.1"', "DEFAULT_MAINNET_PORT": '"10333"'}),
            (r'"127\.0\.0\.1:20333"', 'format!("{}:{}", LOCALHOST, DEFAULT_TESTNET_PORT)', {"LOCALHOST": '"127.0.0.1"', "DEFAULT_TESTNET_PORT": '"20333"'}),
            (r'"127\.0\.0\.1:30333"', 'format!("{}:{}", LOCALHOST, DEFAULT_PRIVNET_PORT)', {"LOCALHOST": '"127.0.0.1"', "DEFAULT_PRIVNET_PORT": '"30333"'}),
            (r'"127\.0\.0\.1:10332"', 'format!("{}:{}", LOCALHOST, DEFAULT_RPC_PORT)', {"LOCALHOST": '"127.0.0.1"', "DEFAULT_RPC_PORT": '"10332"'}),
            (r'"127\.0\.0\.1:10334"', 'format!("{}:{}", LOCALHOST, DEFAULT_WS_PORT)', {"LOCALHOST": '"127.0.0.1"', "DEFAULT_WS_PORT": '"10334"'}),
        ]
        
        original_line = line
        for pattern, replacement, needed_constants in replacements:
            if re.search(pattern, line):
                line = re.sub(pattern, replacement, line)
                constants_needed.update(needed_constants)
                fixed += 1
        
        # Special handling for parse() calls with IPs
        if '.parse().expect(' in line and any(ip in line for ip in ['"0.0.0.0:0"', '"127.0.0.1:']):
            # Already handled above
            pass
        
        lines[i] = line
    
    if fixed > 0:
        # First add constants if needed
        if constants_needed:
            add_constants_to_file(file_path, constants_needed)
            # Re-read file after adding constants
            with open(file_path, 'r') as f:
                lines = f.read().split('\n')
        
        # Apply replacements again after constants are added
        for i, line in enumerate(lines):
            if line.strip().startswith('//'):
                continue
                
            for pattern, replacement, _ in replacements:
                if re.search(pattern, line):
                    line = re.sub(pattern, replacement, line)
            
            lines[i] = line
        
        with open(file_path, 'w') as f:
            f.write('\n'.join(lines))
        
        print(f"Fixed {fixed} hardcoded IP addresses in {file_path}")
    
    return fixed

# Files to fix
files_to_fix = [
    "crates/network/src/error.rs",
    "crates/network/src/lib.rs",
]

# Fix network module constants first
network_lib = "crates/network/src/lib.rs"
if os.path.exists(network_lib):
    # Add network-wide constants
    add_constants_to_file(network_lib, {
        "DEFAULT_MAINNET_PORT": '"10333"',
        "DEFAULT_TESTNET_PORT": '"20333"', 
        "DEFAULT_PRIVNET_PORT": '"30333"',
        "DEFAULT_RPC_PORT": '"10332"',
        "DEFAULT_WS_PORT": '"10334"',
        "LOCALHOST": '"127.0.0.1"',
    })

total_fixed = 0
for file_path in files_to_fix:
    fixed = fix_hardcoded_ips_in_file(file_path)
    total_fixed += fixed

print(f"\nTotal hardcoded IP addresses fixed: {total_fixed}")