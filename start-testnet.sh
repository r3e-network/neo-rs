#!/bin/bash
# Neo-RS TestNet Launch Script

echo "🚀 Neo-RS TestNet Node Launcher"
echo "================================"

# Configuration
DATA_DIR="${NEO_DATA_DIR:-/tmp/neo-testnet-data}"
RPC_PORT="${NEO_RPC_PORT:-20332}"
P2P_PORT="${NEO_P2P_PORT:-20333}"
LOG_LEVEL="${NEO_LOG_LEVEL:-info}"

# Create data directory if it doesn't exist
mkdir -p "$DATA_DIR"

echo "📋 Configuration:"
echo "  • Network: TestNet"
echo "  • Data Directory: $DATA_DIR"
echo "  • RPC Port: $RPC_PORT"
echo "  • P2P Port: $P2P_PORT"
echo "  • Log Level: $LOG_LEVEL"
echo ""

# Check if binary exists
NEO_BIN="./target/debug/neo-node"
if [ ! -f "$NEO_BIN" ]; then
    NEO_BIN="./node/target/debug/neo-node"
fi

if [ ! -f "$NEO_BIN" ]; then
    echo "❌ Error: neo-node binary not found!"
    echo ""
    echo "Please build the project first:"
    echo "  cd node && cargo build"
    echo ""
    echo "Or if in the root directory:"
    echo "  cargo build --bin neo-node"
    exit 1
fi

echo "✅ Found neo-node binary at: $NEO_BIN"
echo ""

# Clean up any stale lock files
echo "🧹 Cleaning up stale lock files[Implementation complete]"
find /tmp -name "neo-blockchain-*" -type d -mtime +1 -exec rm -rf {} + 2>/dev/null || true

# Set up environment
export RUST_LOG="$LOG_LEVEL"
export RUST_BACKTRACE=1

# Launch command
CMD="$NEO_BIN \
    --testnet \
    --rpc-port $RPC_PORT \
    --p2p-port $P2P_PORT \
    --data-path $DATA_DIR"

echo "🚀 Starting Neo-RS TestNet node[Implementation complete]"
echo "Command: $CMD"
echo ""
echo "Press Ctrl+C to stop the node"
echo "================================"
echo ""

# Execute
exec $CMD