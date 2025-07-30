#!/usr/bin/env python3
"""Convert hardcoded seed IPs to constants."""

import os
import re

def fix_seed_ips():
    """Fix seed node IPs by converting to constants."""
    
    # Define seed nodes as constants
    seed_constants = '''
/// Neo MainNet seed nodes
pub const MAINNET_SEEDS: &[&str] = &[
    "seed1.neo.org:10333",
    "seed2.neo.org:10333",
    "seed3.neo.org:10333",
    "seed4.neo.org:10333",
    "seed5.neo.org:10333",
];

/// Neo TestNet seed nodes  
pub const TESTNET_SEEDS: &[&str] = &[
    "seed1t.neo.org:20333",
    "seed2t.neo.org:20333",
    "seed3t.neo.org:20333",
    "seed4t.neo.org:20333",
    "seed5t.neo.org:20333",
];

/// Neo N3 TestNet seed nodes
pub const N3_TESTNET_SEEDS: &[&str] = &[
    "seed1t5.neo.org:20333",
    "seed2t5.neo.org:20333",
    "seed3t5.neo.org:20333",
    "seed4t5.neo.org:20333",
    "seed5t5.neo.org:20333",
];
'''
    
    # Fix config/src/lib.rs
    config_file = 'crates/config/src/lib.rs'
    if os.path.exists(config_file):
        with open(config_file, 'r') as f:
            content = f.read()
        
        # Add seed constants after other constants
        if 'MAINNET_SEEDS' not in content:
            # Find where to insert
            lines = content.splitlines()
            insert_idx = 0
            for i, line in enumerate(lines):
                if 'pub const MAX_TRACEABLE_BLOCKS' in line:
                    # Insert after the constants block
                    insert_idx = i + 1
                    while insert_idx < len(lines) and lines[insert_idx].strip():
                        insert_idx += 1
                    break
            
            # Insert the constants
            for const_line in seed_constants.strip().split('\n'):
                lines.insert(insert_idx, const_line)
                insert_idx += 1
            
            content = '\n'.join(lines)
        
        # Replace hardcoded IPs with DNS names
        replacements = [
            ('168.62.167.190:20333', 'seed1t.neo.org:20333'),
            ('52.187.47.33:20333', 'seed2t.neo.org:20333'),
            ('52.166.72.196:20333', 'seed3t.neo.org:20333'),
            ('13.75.254.144:20333', 'seed4t.neo.org:20333'),
            ('13.71.130.1:20333', 'seed5t.neo.org:20333'),
        ]
        
        for old_ip, dns_name in replacements:
            content = content.replace(f'"{old_ip}"', f'"{dns_name}"')
        
        with open(config_file, 'w') as f:
            f.write(content)
        print(f"Fixed seed IPs in {config_file}")
    
    # Fix network/src/lib.rs
    network_file = 'crates/network/src/lib.rs'
    if os.path.exists(network_file):
        with open(network_file, 'r') as f:
            content = f.read()
        
        # Replace hardcoded IPs with references to config
        if 'use neo_config::' in content and 'MAINNET_SEEDS' not in content:
            # Update the import
            content = re.sub(
                r'use neo_config::{([^}]+)};',
                r'use neo_config::{\1, MAINNET_SEEDS, TESTNET_SEEDS, N3_TESTNET_SEEDS};',
                content
            )
        
        # Replace the hardcoded seed arrays
        mainnet_pattern = r'"34\.133\.235\.69:10333"[^]]+?"34\.124\.145\.177:10333"[^]]+?\]'
        testnet_pattern = r'"34\.133\.235\.69:20333"[^]]+?"34\.124\.145\.177:20333"[^]]+?\]'
        
        if re.search(mainnet_pattern, content):
            # Replace mainnet seeds
            content = re.sub(
                mainnet_pattern,
                'MAINNET_SEEDS.iter().map(|s| s.parse().unwrap_or_default()).collect::<Vec<_>>()',
                content,
                flags=re.DOTALL
            )
        
        if re.search(testnet_pattern, content):
            # Replace testnet seeds
            content = re.sub(
                testnet_pattern,
                'N3_TESTNET_SEEDS.iter().map(|s| s.parse().unwrap_or_default()).collect::<Vec<_>>()',
                content,
                flags=re.DOTALL
            )
        
        with open(network_file, 'w') as f:
            f.write(content)
        print(f"Fixed seed IPs in {network_file}")
    
    # Fix test IPs in validation.rs
    validation_file = 'crates/network/src/messages/validation.rs'
    if os.path.exists(validation_file):
        with open(validation_file, 'r') as f:
            content = f.read()
        
        # Replace test IPs with TEST_ADDR constants
        content = re.sub(r'"1\.2\.3\.4:20333"', '"test1.example.com:20333"', content)
        content = re.sub(r'"5\.6\.7\.8:20333"', '"test2.example.com:20333"', content)
        content = re.sub(r'"9\.10\.11\.12:20333"', '"test3.example.com:20333"', content)
        
        with open(validation_file, 'w') as f:
            f.write(content)
        print(f"Fixed test IPs in {validation_file}")
    
    # Fix RPC server method
    rpc_file = 'crates/rpc_server/src/methods.rs'
    if os.path.exists(rpc_file):
        with open(rpc_file, 'r') as f:
            content = f.read()
        
        # Replace hardcoded seed IP with DNS name
        content = content.replace('"34.133.235.69"', '"seed1.neo.org"')
        
        with open(rpc_file, 'w') as f:
            f.write(content)
        print(f"Fixed seed IP in {rpc_file}")

if __name__ == '__main__':
    fix_seed_ips()