#!/bin/bash
# Test script to verify height parsing fix

echo "Starting neo-cli to test height parsing..."
./target/debug/neo-cli &
PID=$!

# Wait for connections
sleep 20

# Kill the process
kill $PID 2>/dev/null || true

echo "Test completed."