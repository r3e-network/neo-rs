#!/bin/bash
# Continuous monitoring script for neo-rs node

SERVER="root@89.167.120.122"
SSH_KEY="$HOME/.ssh/id_ed25519"

echo "=== Neo-RS Continuous Monitor ==="
echo "Started at: $(date)"
echo ""

while true; do
    echo "----------------------------------------"
    echo "Check at: $(date)"

    # 1. Process status
    PROCESS=$(ssh -i "$SSH_KEY" "$SERVER" "ps aux | awk '/neo-node/ && !/awk/' | wc -l")
    if [ "$PROCESS" -gt 0 ]; then
        echo "✓ Process: Running"
        MEM=$(ssh -i "$SSH_KEY" "$SERVER" "ps aux | awk '/neo-node/ && !/awk/ {print \$6}' | head -1")
        echo "  Memory: $((MEM/1024)) MB"
    else
        echo "✗ Process: NOT RUNNING"
    fi

    # 2. Block height
    HEIGHT=$(ssh -i "$SSH_KEY" "$SERVER" "curl -s http://localhost:10332 -H 'Content-Type: application/json' -d '{\"jsonrpc\":\"2.0\",\"method\":\"getblockcount\",\"params\":[],\"id\":1}' | strings | grep -o '\"result\":[0-9]*' | cut -d: -f2")
    if [ -n "$HEIGHT" ]; then
        echo "✓ Block Height: $HEIGHT"
    else
        echo "✗ Block Height: Failed to fetch"
    fi

    # 3. Peer connections
    PEERS=$(ssh -i "$SSH_KEY" "$SERVER" "curl -s http://localhost:10332 -H 'Content-Type: application/json' -d '{\"jsonrpc\":\"2.0\",\"method\":\"getconnectioncount\",\"params\":[],\"id\":1}' | strings | grep -o '\"result\":[0-9]*' | cut -d: -f2")
    if [ -n "$PEERS" ]; then
        echo "✓ Peers: $PEERS"
    else
        echo "✗ Peers: Failed to fetch"
    fi

    # 4. Recent sync activity
    RECENT_BLOCKS=$(ssh -i "$SSH_KEY" "$SERVER" "tail -100 /root/neo-node-optimized.log | grep 'inventory completed' | grep 'has_block=true' | wc -l")
    echo "  Recent blocks received: $RECENT_BLOCKS"

    # 5. Active tasks
    ACTIVE_TASKS=$(ssh -i "$SSH_KEY" "$SERVER" "tail -50 /root/neo-node-optimized.log | grep 'requesting tasks' | tail -1 | grep -o 'index_tasks=[0-9]*' | cut -d= -f2")
    if [ -n "$ACTIVE_TASKS" ]; then
        echo "  Active download tasks: $ACTIVE_TASKS"
    fi

    echo ""
    sleep 120  # Check every 2 minutes
done
