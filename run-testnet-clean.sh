#!/bin/bash

# Neo-RS TestNet Node Launcher
# This script safely starts the Neo-RS node on TestNet with proper cleanup

set -e  # Exit on any error

echo "🚀 Neo-RS TestNet Node Launcher"
echo "==============================="

# Function to cleanup and exit gracefully
cleanup() {
    echo ""
    echo "🛑 Shutting down Neo-RS node[Implementation complete]"
    pkill -f neo-node 2>/dev/null || true
    sleep 2
    echo "✅ Cleanup completed"
    exit 0
}

# Set up signal handlers
trap cleanup SIGINT SIGTERM

# Step 1: Kill any existing processes
echo "🧹 Cleaning up existing processes[Implementation complete]"
pkill -f neo-node 2>/dev/null || true
sleep 1

# Step 2: Clean up lock files and temporary data
echo "🗂️  Cleaning up data directories[Implementation complete]"
rm -rf /tmp/neo-blockchain-* 2>/dev/null || true
rm -rf /Users/jinghuiliao/.neo-rs/testnet/LOCK 2>/dev/null || true
rm -rf ./data 2>/dev/null || true

# Step 3: Ensure binary exists
if [ ! -f "./target/release/neo-node" ]; then
    echo "❌ Neo-node binary not found. Building[Implementation complete]"
    cargo build --release
fi

# Step 4: Start the node
echo ""
echo "🌐 Starting Neo-RS node on TestNet[Implementation complete]"
echo "📊 Configuration:"
echo "   ├─ Network: TestNet" 
echo "   ├─ RPC Port: 20332"
echo "   ├─ P2P Port: 20333"
echo "   └─ Data Path: ~/.neo-rs/testnet"
echo ""

# Set environment variables
export RUST_LOG=info
export NEO_SKIP_LEDGER_INIT=1

# Run the node with proper error handling
echo "⏳ Launching node[Implementation complete]"
./target/release/neo-node --testnet --rpc-port 20332 --p2p-port 20333 &
NODE_PID=$!

echo "🎯 Node started with PID: $NODE_PID"
echo ""
echo "🔗 TestNet Services:"
echo "   RPC API: http://localhost:20332"
echo "   Status:  Node syncing with TestNet[Implementation complete]"
echo ""
echo "📝 Logs will appear below (Press Ctrl+C to stop):"
echo "=================================================="

# Wait for the node process
wait $NODE_PID