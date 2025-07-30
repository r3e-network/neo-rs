#!/bin/bash

# Clean up any existing data for fresh start
rm -rf ~/.neo-rs/testnet

# Set up logging
export RUST_LOG=info,neo_network=debug,neo_ledger=debug,neo_consensus=debug

# Create log directory
mkdir -p logs

# Run the node with logging
echo "Starting Neo-RS node on TestNet..."
echo "Logs will be written to logs/neo-testnet.log"
echo "Press Ctrl+C to stop"
echo ""

# Run node and tee output to both console and file
./target/release/neo-node --testnet 2>&1 | tee logs/neo-testnet.log