#!/bin/bash

# Safe Node Startup Script - Avoids double blockchain initialization
set -e

echo "=== Safe Neo-RS Node Startup ==="

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Configuration
DATA_DIR="$HOME/.neo-rs-$(date +%s)"
RPC_PORT=30332
P2P_PORT=30334
LOG_FILE="neo-node-safe.log"

echo "Starting with configuration:"
echo "  Data Directory: $DATA_DIR"
echo "  RPC Port: $RPC_PORT"
echo "  P2P Port: $P2P_PORT"
echo "  Log File: $LOG_FILE"
echo

# Clean any existing state
echo "1. Cleaning previous state[Implementation complete]"
pkill -f neo-node || true
sleep 2
rm -rf /tmp/neo-blockchain-* ~/.neo-rs-*
mkdir -p "$DATA_DIR"

# Start node with minimal environment
echo "2. Starting node[Implementation complete]"
unset HTTP_PROXY HTTPS_PROXY

# Start with one-shot approach - if it fails quickly, we'll know why
if ./target/release/neo-node --testnet --data-path "$DATA_DIR" --rpc-port $RPC_PORT --p2p-port $P2P_PORT > "$LOG_FILE" 2>&1 &
then
    NODE_PID=$!
    echo "Node started with PID: $NODE_PID"
    
    # Monitor for first 30 seconds
    echo "3. Monitoring startup for 30 seconds[Implementation complete]"
    for i in {1..30}; do
        if ! kill -0 $NODE_PID 2>/dev/null; then
            echo -e "${RED}✗ Node crashed after $i seconds${NC}"
            echo "Last few lines of log:"
            tail -10 "$LOG_FILE"
            exit 1
        fi
        
        # Check for successful startup indicators
        if grep -q "Neo-Rust node started successfully" "$LOG_FILE" 2>/dev/null; then
            echo -e "${GREEN}✓ Node started successfully${NC}"
            break
        fi
        
        # Check for double blockchain error
        if grep -q "Failed to create fallback RocksDB storage" "$LOG_FILE" 2>/dev/null; then
            echo -e "${RED}✗ Double blockchain initialization detected${NC}"
            kill $NODE_PID 2>/dev/null || true
            
            echo "This is a code architecture issue. Here's what's happening:"
            grep -A 2 -B 2 "Creating new blockchain instance" "$LOG_FILE" || true
            echo
            echo "Workaround: The RPC server is trying to create a second blockchain instance."
            echo "This needs to be fixed in the source code."
            exit 1
        fi
        
        sleep 1
    done
    
    # If we got here, check final status
    if kill -0 $NODE_PID 2>/dev/null; then
        echo -e "${GREEN}✓ Node is running successfully${NC}"
        echo "  PID: $NODE_PID"
        echo "  RPC: http://localhost:$RPC_PORT/rpc"
        echo "  P2P: $P2P_PORT"
        echo "  Logs: $LOG_FILE"
        
        # Save PID for management
        echo $NODE_PID > neo-node.pid
        
        # Test RPC quickly
        sleep 2
        if curl -s -X POST http://localhost:$RPC_PORT/rpc \
           -H "Content-Type: application/json" \
           -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}' | grep -q "result"; then
            echo -e "${GREEN}✓ RPC is responding${NC}"
        else
            echo -e "${YELLOW}⚠ RPC not yet responding${NC}"
        fi
        
        # Check P2P binding
        if lsof -i :$P2P_PORT >/dev/null 2>&1; then
            echo -e "${GREEN}✓ P2P port is bound${NC}"
        else
            echo -e "${RED}✗ P2P port not bound${NC}"
        fi
        
        echo
        echo "Monitor with: tail -f $LOG_FILE"
        echo "Test with: ./test-neo-node.sh"
    else
        echo -e "${RED}✗ Node stopped unexpectedly${NC}"
        echo "Check $LOG_FILE for details"
        exit 1
    fi
else
    echo -e "${RED}✗ Failed to start node${NC}"
    exit 1
fi