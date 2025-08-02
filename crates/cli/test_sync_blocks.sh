#!/bin/bash
# Test script to check if blocks are being synchronized

echo "Starting neo-cli to test block synchronization..."
./target/debug/neo-cli 2>&1 | grep -E "(GetHeaders|Requested headers|Received.*headers|GetBlocks|Block height|Requesting blocks|sync)" &
PID=$!

# Wait for synchronization
sleep 30

# Kill the process
kill $PID 2>/dev/null || true

echo "Test completed."