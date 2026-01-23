#!/bin/bash

# Neo Rust Node - TestNet Query Examples

RPC_URL="http://127.0.0.1:20332"

echo "🔍 Neo TestNet Query Examples"
echo "============================="

# Function to make RPC calls
rpc_call() {
    curl -s -X POST "$RPC_URL" \
        -H "Content-Type: application/json" \
        -d "$1" | jq '.'
}

echo ""
echo "📊 Node Status:"
rpc_call '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}'

echo ""
echo "🔗 Network Info:"
rpc_call '{"jsonrpc":"2.0","method":"getconnectioncount","params":[],"id":1}'

echo ""
echo "📦 Latest Block:"
rpc_call '{"jsonrpc":"2.0","method":"getblockcount","params":[],"id":1}'

echo ""
echo "🏷️ Best Block Hash:"
rpc_call '{"jsonrpc":"2.0","method":"getbestblockhash","params":[],"id":1}'

echo ""
echo "💰 GAS Token Info:"
rpc_call '{"jsonrpc":"2.0","method":"getnep17balances","params":["NiNmXL8FjEUEs1nfX9uHFBNaenxDHJtmuB"],"id":1}'

echo ""
echo "🔍 Query specific block (block 1):"
rpc_call '{"jsonrpc":"2.0","method":"getblock","params":[1, true],"id":1}'
