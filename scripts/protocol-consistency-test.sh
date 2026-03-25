#!/bin/bash
# Neo-RS Protocol Consistency Test Suite

SERVER="root@89.167.120.122"
SSH_KEY="~/.ssh/id_ed25519"
RPC="http://localhost:10332"

echo "=== Neo-RS Protocol Consistency Tests ==="
echo "Time: $(date)"
echo ""

# Test 1: Block Hash Consistency
echo "Test 1: Block Hash Verification"
BLOCKS=(1000 5000 10000 15000 20000)
for HEIGHT in "${BLOCKS[@]}"; do
    HASH=$(ssh -i $SSH_KEY $SERVER "curl -s --compressed -X POST $RPC -H 'Content-Type: application/json' -d '{\"jsonrpc\":\"2.0\",\"method\":\"getblock\",\"params\":[$HEIGHT,1],\"id\":1}' | jq -r '.result.hash'")
    echo "  Block $HEIGHT: $HASH"
done
echo ""

# Test 2: Network Protocol
echo "Test 2: Network Protocol Info"
ssh -i $SSH_KEY $SERVER "curl -s --compressed -X POST $RPC -H 'Content-Type: application/json' -d '{\"jsonrpc\":\"2.0\",\"method\":\"getversion\",\"params\":[],\"id\":1}' | jq '.result.protocol | {network, msperblock, maxtraceableblocks}'"
echo ""

# Test 3: State Root (if available)
echo "Test 3: State Root Check"
ssh -i $SSH_KEY $SERVER "curl -s --compressed -X POST $RPC -H 'Content-Type: application/json' -d '{\"jsonrpc\":\"2.0\",\"method\":\"getstateroot\",\"params\":[1000],\"id\":1}' | jq '.result // \"Not available\"'"
echo ""

# Test 4: Connection Health
echo "Test 4: Network Health"
CONN=$(ssh -i $SSH_KEY $SERVER "curl -s --compressed -X POST $RPC -H 'Content-Type: application/json' -d '{\"jsonrpc\":\"2.0\",\"method\":\"getconnectioncount\",\"params\":[],\"id\":1}' | jq -r '.result'")
echo "  Connections: $CONN"
if [ "$CONN" -ge 5 ]; then
    echo "  Status: ✅ Healthy"
else
    echo "  Status: ⚠️  Low connections"
fi
echo ""

echo "=== Tests Complete ==="
