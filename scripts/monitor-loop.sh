#!/bin/bash
# Continuous monitoring loop for Neo-RS mainnet node

INTERVAL=60  # Check every 60 seconds
LOG_FILE="/tmp/neo-validation.log"

while true; do
    echo "=== $(date) ===" | tee -a $LOG_FILE

    # Quick status check
    BLOCK=$(ssh -i ~/.ssh/id_ed25519 root@89.167.120.122 "curl -s --compressed -X POST http://localhost:10332 -H 'Content-Type: application/json' -d '{\"jsonrpc\":\"2.0\",\"method\":\"getblockcount\",\"params\":[],\"id\":1}' | jq -r '.result'")
    CONN=$(ssh -i ~/.ssh/id_ed25519 root@89.167.120.122 "curl -s --compressed -X POST http://localhost:10332 -H 'Content-Type: application/json' -d '{\"jsonrpc\":\"2.0\",\"method\":\"getconnectioncount\",\"params\":[],\"id\":1}' | jq -r '.result'")

    echo "Block: $BLOCK | Connections: $CONN" | tee -a $LOG_FILE

    sleep $INTERVAL
done
