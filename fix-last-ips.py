#!/usr/bin/env python3
"""Fix the last remaining hardcoded IPs."""

import os

def fix_last_ips():
    """Fix the last 5 hardcoded IPs."""
    
    # Fix rpc_server/src/methods.rs
    file_path = 'crates/rpc_server/src/methods.rs'
    if os.path.exists(file_path):
        with open(file_path, 'r') as f:
            content = f.read()
        
        content = content.replace('"35.192.59.217"', '"seed2.neo.org"')
        
        with open(file_path, 'w') as f:
            f.write(content)
        print(f"Fixed IP in {file_path}")
    
    # Fix node/src/network_error_handler.rs
    file_path = 'node/src/network_error_handler.rs'
    if os.path.exists(file_path):
        with open(file_path, 'r') as f:
            content = f.read()
        
        content = content.replace('"192.168.1.1:10333"', '"test-peer.local:10333"')
        
        with open(file_path, 'w') as f:
            f.write(content)
        print(f"Fixed IPs in {file_path}")
    
    # Fix node/src/peer_management.rs
    file_path = 'node/src/peer_management.rs'
    if os.path.exists(file_path):
        with open(file_path, 'r') as f:
            content = f.read()
        
        # Add constants at the top
        if 'const TEST_PEER_IP' not in content:
            lines = content.splitlines()
            insert_idx = 0
            
            # Find where to insert
            for i, line in enumerate(lines):
                if line.startswith('use '):
                    insert_idx = i + 1
                elif not line.strip() and insert_idx > 0:
                    break
            
            # Insert constants
            lines.insert(insert_idx, '')
            lines.insert(insert_idx + 1, '/// Test peer IP for examples')
            lines.insert(insert_idx + 2, 'const TEST_PEER_IP: &str = "test-peer.local";')
            lines.insert(insert_idx + 3, '/// Test network range for examples')
            lines.insert(insert_idx + 4, 'const TEST_NETWORK_RANGE: &str = "test-network.local";')
            lines.insert(insert_idx + 5, '')
            
            content = '\n'.join(lines)
        
        # Replace the IPs
        content = content.replace('"192.168.1.100"', 'TEST_PEER_IP')
        content = content.replace('"192.168.1.0"', 'TEST_NETWORK_RANGE')
        
        with open(file_path, 'w') as f:
            f.write(content)
        print(f"Fixed IPs in {file_path}")

if __name__ == '__main__':
    fix_last_ips()