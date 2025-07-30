#!/bin/bash
set -e

# Neo-RS Docker Entrypoint Script
# Handles initialization, configuration, and startup

echo "=== Neo-RS Docker Container Starting ==="
echo "Timestamp: $(date)"
echo "User: $(whoami)"
echo "Working Directory: $(pwd)"

# Environment variable defaults
NEO_NETWORK=${NEO_NETWORK:-testnet}
NEO_RPC_PORT=${NEO_RPC_PORT:-30332}
NEO_P2P_PORT=${NEO_P2P_PORT:-30334}
NEO_DATA_PATH=${NEO_DATA_PATH:-/opt/neo-rs/data}
NEO_LOG_LEVEL=${NEO_LOG_LEVEL:-info}
NEO_MAX_PEERS=${NEO_MAX_PEERS:-100}
NEO_RPC_BIND=${NEO_RPC_BIND:-0.0.0.0}

# Create data directories if they don't exist
mkdir -p "$NEO_DATA_PATH"
mkdir -p /opt/neo-rs/logs

# Configuration summary
echo "Configuration:"
echo "  Network: $NEO_NETWORK"
echo "  RPC Port: $NEO_RPC_PORT"
echo "  P2P Port: $NEO_P2P_PORT"
echo "  Data Path: $NEO_DATA_PATH"
echo "  Log Level: $NEO_LOG_LEVEL"
echo "  Max Peers: $NEO_MAX_PEERS"
echo "  RPC Bind: $NEO_RPC_BIND"

# Check binary exists and is executable
if [ ! -x "/opt/neo-rs/bin/neo-node" ]; then
    echo "ERROR: neo-node binary not found or not executable"
    exit 1
fi

# Health check function
health_check() {
    echo "Performing initial health check[Implementation complete]"
    sleep 5  # Give service time to start
    
    for i in {1..12}; do  # Try for 60 seconds
        if curl -s --connect-timeout 5 --max-time 10 \
           -X POST http://localhost:$NEO_RPC_PORT/rpc \
           -H "Content-Type: application/json" \
           -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}' > /dev/null 2>&1; then
            echo "✅ RPC endpoint healthy"
            return 0
        fi
        echo "Waiting for RPC endpoint[Implementation complete] ($i/12)"
        sleep 5
    done
    
    echo "⚠️ RPC endpoint not responding after 60 seconds"
    return 1
}

# Signal handlers
shutdown() {
    echo "Received shutdown signal"
    if [ -n "$NEO_PID" ]; then
        echo "Stopping neo-node (PID: $NEO_PID)"
        kill -TERM "$NEO_PID"
        wait "$NEO_PID"
    fi
    echo "Neo-RS container stopped"
    exit 0
}

# Trap signals
trap shutdown SIGTERM SIGINT

# Build command line arguments
ARGS=()

# Network selection
case "$NEO_NETWORK" in
    "testnet")
        ARGS+=("--testnet")
        ;;
    "mainnet")
        ARGS+=("--mainnet")
        ;;
    *)
        echo "WARNING: Unknown network '$NEO_NETWORK', defaulting to testnet"
        ARGS+=("--testnet")
        ;;
esac

# Add standard arguments
ARGS+=("--data-path" "$NEO_DATA_PATH")
ARGS+=("--rpc-port" "$NEO_RPC_PORT")
ARGS+=("--p2p-port" "$NEO_P2P_PORT")

# Add any additional arguments passed to the container
ARGS+=("$@")

# Log the command that will be executed
echo "Executing: /opt/neo-rs/bin/neo-node ${ARGS[*]}"

# Start neo-node in background
/opt/neo-rs/bin/neo-node "${ARGS[@]}" > /opt/neo-rs/logs/neo-node.log 2>&1 &
NEO_PID=$!

echo "Neo-RS started with PID: $NEO_PID"

# Perform health check
health_check &

# Wait for the process
wait "$NEO_PID"

echo "Neo-RS process exited"