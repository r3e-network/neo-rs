#!/bin/bash
# Neo-RS Mainnet Continuous Validation Script

SERVER="root@89.167.120.122"
SSH_KEY="~/.ssh/id_ed25519"
RPC_URL="http://localhost:10332"

echo "=== Neo-RS Mainnet Validation ==="
echo "Time: $(date)"
echo ""

# 1. Process Status
echo "1. Process Status:"
PROCESS_COUNT=$(ssh -i $SSH_KEY $SERVER "pgrep -c neo-node")
if [ "$PROCESS_COUNT" -gt 0 ]; then
    echo "   ✅ Node running (PID count: $PROCESS_COUNT)"
else
    echo "   ❌ Node NOT running"
    exit 1
fi
echo ""

# 2. Block Height
echo "2. Block Sync:"
BLOCK_HEIGHT=$(ssh -i $SSH_KEY $SERVER "curl -s --compressed -X POST $RPC_URL -H 'Content-Type: application/json' -d '{\"jsonrpc\":\"2.0\",\"method\":\"getblockcount\",\"params\":[],\"id\":1}' | jq -r '.result'")
echo "   Current height: $BLOCK_HEIGHT"
echo ""

# 3. Network Connections
echo "3. P2P Network:"
CONNECTIONS=$(ssh -i $SSH_KEY $SERVER "curl -s --compressed -X POST $RPC_URL -H 'Content-Type: application/json' -d '{\"jsonrpc\":\"2.0\",\"method\":\"getconnectioncount\",\"params\":[],\"id\":1}' | jq -r '.result'")
echo "   Active connections: $CONNECTIONS"
echo ""

# 4. Block Validation
echo "4. Block Validation:"
BLOCK_1000=$(ssh -i $SSH_KEY $SERVER "curl -s --compressed -X POST $RPC_URL -H 'Content-Type: application/json' -d '{\"jsonrpc\":\"2.0\",\"method\":\"getblock\",\"params\":[1000,1],\"id\":1}' | jq -r '.result.hash'")
EXPECTED_HASH="0xe31ad93809a2ac112b066e50a72ad4883cf9f94a155a7dea2f05e69417b2b9aa"
if [ "$BLOCK_1000" == "$EXPECTED_HASH" ]; then
    echo "   ✅ Block #1000 hash verified"
else
    echo "   ❌ Block #1000 hash mismatch"
    echo "      Expected: $EXPECTED_HASH"
    echo "      Got: $BLOCK_1000"
fi
echo ""

# 5. Transaction Execution Test
echo "5. Transaction Execution:"
BEST_BLOCK=$(ssh -i $SSH_KEY $SERVER "curl -s --compressed -X POST $RPC_URL -H 'Content-Type: application/json' -d '{\"jsonrpc\":\"2.0\",\"method\":\"getbestblockhash\",\"params\":[],\"id\":1}' | jq -r '.result'")
echo "   Best block hash: $BEST_BLOCK"
echo ""

# 6. Memory and Disk
echo "6. System Resources:"
ssh -i $SSH_KEY $SERVER "free -h | grep Mem | awk '{print \"   Memory: \" \$3 \" / \" \$2}'"
ssh -i $SSH_KEY $SERVER "df -h /root/neo-data/mainnet | tail -1 | awk '{print \"   Disk used: \" \$3 \" / \" \$2 \" (\" \$5 \")\"}'"
echo ""

echo "=== Validation Complete ==="
