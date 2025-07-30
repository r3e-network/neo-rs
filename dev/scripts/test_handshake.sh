#!/bin/bash

echo "Testing Neo handshake improvements[Implementation complete]"
echo "===================================="

# Run for 15 seconds and capture output
./target/debug/neo-node --testnet 2>&1 | head -100 &
NODE_PID=$!

# Wait for a bit
sleep 15

# Kill the node
kill $NODE_PID 2>/dev/null

echo ""
echo "Test completed."