#!/bin/bash
# Neo Rust TestNet Node Startup Script

set -e

echo "🚀 Starting Neo Rust TestNet Node"
echo "=================================="

# Configuration
DATA_DIR="/tmp/neo-testnet-production"
LOG_LEVEL="info"
NODE_BINARY="./target/release/neo-node"

# Create data directory
mkdir -p "$DATA_DIR"
echo "📁 Data directory: $DATA_DIR"

# Check binary exists
if [ ! -f "$NODE_BINARY" ]; then
    echo "❌ Neo node binary not found at $NODE_BINARY"
    echo "   Run: cargo build --release"
    exit 1
fi

echo "✅ Neo node binary found: $NODE_BINARY"

# Set environment variables
export RUST_LOG="$LOG_LEVEL"
export RUST_BACKTRACE=1

# Node information
echo "🌐 Network: TestNet"
echo "📡 P2P Port: 20333" 
echo "🔌 RPC Port: 20332"
echo "💾 Storage: $DATA_DIR"

# Start node
echo "🚀 Launching node..."
echo "   Log Level: $LOG_LEVEL"
echo "   Press Ctrl+C to stop"
echo ""

# Run with full logging
exec "$NODE_BINARY" \
    --testnet \
    --data-dir "$DATA_DIR" \
    2>&1 | tee "$DATA_DIR/node.log"