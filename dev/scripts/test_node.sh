#!/bin/bash
# Simple test script to run the node and check basic functionality

echo "🚀 Starting Neo-Rust node test..."
echo "================================"

# Run the node for 30 seconds
timeout 30s ./target/debug/neo-node --testnet 2>&1 | while read line; do
    echo "$line"
    
    # Check for important events
    if [[ "$line" == *"Connected to peer"* ]]; then
        echo "✅ SUCCESS: Connected to a peer!"
    fi
    
    if [[ "$line" == *"Handshake completed"* ]]; then
        echo "✅ SUCCESS: Handshake completed!"
    fi
    
    if [[ "$line" == *"Height:"* ]] && [[ "$line" == *"(+"* ]]; then
        if [[ "$line" != *"(+0"* ]]; then
            echo "✅ SUCCESS: Blockchain is syncing!"
        fi
    fi
done

echo ""
echo "Test completed. Check above for any SUCCESS messages."