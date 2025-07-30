#!/bin/bash

echo "Testing Neo node with corrected protocol magic numbers[Implementation complete]"
echo "========================================================="

# Run for 20 seconds and capture output
./target/debug/neo-node --testnet 2>&1 | head -150 &
NODE_PID=$!

# Wait for a bit
sleep 20

# Kill the node
kill $NODE_PID 2>/dev/null

echo ""
echo "Test completed with corrected magic numbers."