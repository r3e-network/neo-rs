#!/bin/bash

# Kill any existing neo-node processes
pkill -f neo-node

# Clean up any existing temp directories
rm -rf /tmp/neo-blockchain-*

# Run the neo-node with testnet configuration
echo "Starting Neo-Rust node on TestNet[Implementation complete]"
echo "RPC Port: 20332"
echo "P2P Port: 20333"
echo ""

# Set environment variables to suppress double initialization
export NEO_SKIP_LEDGER_INIT=1

# Run the node
./target/release/neo-node --testnet --rpc-port 20332 --p2p-port 20333