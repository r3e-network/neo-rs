#!/bin/bash

# Neo Rust Node TestNet Monitoring Script

echo "🌐 Neo Rust Node - TestNet Sync Monitor"
echo "======================================="

RPC_URL="http://127.0.0.1:20332"

# Function to query RPC
query_rpc() {
    local method="$1"
    local params="$2"
    
    curl -s -X POST "$RPC_URL" \
        -H "Content-Type: application/json" \
        -d "{\"jsonrpc\":\"2.0\",\"method\":\"$method\",\"params\":$params,\"id\":1}" \
        | jq -r '.result'
}

# Monitor sync status
while true; do
    echo ""
    echo "📊 $(date): Checking sync status..."
    
    # Get current block count
    BLOCK_COUNT=$(query_rpc "getblockcount" "[]")
    
    # Get best block hash
    BEST_HASH=$(query_rpc "getbestblockhash" "[]")
    
    # Get connection count
    CONNECTIONS=$(query_rpc "getconnectioncount" "[]")
    
    # Get version info
    VERSION=$(query_rpc "getversion" "[]" | jq -r '.useragent')
    
    echo "🔗 Connections: $CONNECTIONS"
    echo "📦 Block Height: $BLOCK_COUNT"
    echo "🔍 Best Block: ${BEST_HASH:0:16}..."
    echo "🚀 Version: $VERSION"
    
    # Check if syncing
    if [ "$BLOCK_COUNT" != "null" ] && [ "$BLOCK_COUNT" -gt 0 ]; then
        echo "✅ Node is syncing blocks!"
    else
        echo "⏳ Waiting for sync to start..."
    fi
    
    sleep 10
done
