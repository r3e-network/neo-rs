#!/bin/bash

# Test Neo-RS node with verbose logging for debugging

echo "🔍 Testing Neo-RS Node with Verbose Logging"
echo "==========================================="

# Clean up any existing processes
pkill -f neo-node 2>/dev/null || true
sleep 1

# Clean up data
rm -rf /tmp/neo-blockchain-* 2>/dev/null || true
rm -rf ./data 2>/dev/null || true

# Set verbose logging
export RUST_LOG=debug

echo "🚀 Starting Neo-RS node with debug logging[Implementation complete]"
echo "📍 Watch for connection attempts and handshake details"
echo ""

# Run node for 60 seconds then stop
timeout 60s ./target/release/neo-node \
    --testnet \
    --rpc-port 20332 \
    --p2p-port 20333 \
    --data-path /tmp/neo-test-verbose &

NODE_PID=$!
echo "🎯 Node started with PID: $NODE_PID"

# Wait a bit then test RPC
sleep 10

echo ""
echo "🔧 Testing RPC server[Implementation complete]"
curl -s -X POST \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getblockcount","params":[],"id":1}' \
  http://localhost:20332 || echo "RPC test failed"

echo ""
echo "🔧 Testing health endpoint[Implementation complete]"
curl -s http://localhost:20332/health || echo "Health endpoint test failed"

# Wait for the timeout
wait $NODE_PID

echo ""
echo "✅ Test completed"