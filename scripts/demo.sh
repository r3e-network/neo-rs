#!/bin/bash
# Neo-rs Professional CLI Demo Script

set -e

echo "🚀 Neo-rs Professional CLI Demo"
echo "================================"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Check if the node binary exists
if [ ! -f "./target/release/neo-node" ]; then
    echo -e "${RED}❌ Binaries not found. Building...${NC}"
    cargo build --release
fi

echo -e "${BLUE}📋 Available Commands:${NC}"
echo "1. Start TestNet node"
echo "2. Start MainNet node" 
echo "3. Start local development node"
echo "4. Show node configuration"
echo "5. Validate configuration"
echo "6. CLI operations demo"
echo "7. Docker demo"
echo "8. Exit"

while true; do
    echo ""
    read -p "Select option (1-8): " choice
    
    case $choice in
        1)
            echo -e "${GREEN}🌐 Starting TestNet node...${NC}"
            echo "Command: NEO_NETWORK=testnet NEO_DATA_DIR=./data/testnet ./target/release/neo-node"
            echo "Press Ctrl+C to stop the node"
            NEO_NETWORK=testnet NEO_DATA_DIR=./data/testnet ./target/release/neo-node
            ;;
        2)
            echo -e "${GREEN}🌍 Starting MainNet node...${NC}"
            echo "Command: NEO_NETWORK=mainnet NEO_DATA_DIR=./data/mainnet ./target/release/neo-node"
            echo "Press Ctrl+C to stop the node"
            NEO_NETWORK=mainnet NEO_DATA_DIR=./data/mainnet ./target/release/neo-node
            ;;
        3)
            echo -e "${GREEN}🏠 Starting local development node...${NC}"
            echo "Command: NEO_NETWORK=local NEO_DATA_DIR=./data/local NEO_LOG_LEVEL=debug ./target/release/neo-node"
            echo "Press Ctrl+C to stop the node"
            NEO_NETWORK=local NEO_DATA_DIR=./data/local NEO_LOG_LEVEL=debug ./target/release/neo-node
            ;;
        4)
            echo -e "${BLUE}⚙️  Node Configuration:${NC}"
            echo ""
            echo "TestNet Configuration:"
            echo "----------------------"
            NEO_NETWORK=testnet ./target/release/neo-node config
            echo ""
            echo "MainNet Configuration:"
            echo "----------------------"
            NEO_NETWORK=mainnet ./target/release/neo-node config
            ;;
        5)
            echo -e "${BLUE}✅ Configuration Validation:${NC}"
            if [ -f "config/testnet.toml" ]; then
                echo "Validating TestNet config..."
                ./target/release/neo-node validate config/testnet.toml
            fi
            if [ -f "config/mainnet.toml" ]; then
                echo "Validating MainNet config..."
                ./target/release/neo-node validate config/mainnet.toml
            fi
            ;;
        6)
            echo -e "${BLUE}🔧 RPC Operations Demo:${NC}"
            echo ""
            echo "Available queries (requires a running node on :10332):"
            echo "• getblockcount - Get block height"
            echo "• getblock       - Get block info"
            echo "• getpeers      - Get peer info"
            echo ""
            echo "Example usage (curl against the node's JSON-RPC):"
            echo "curl -s http://127.0.0.1:10332 -H 'Content-Type: application/json' \\"
            echo "  -d '{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getblockcount\",\"params\":[]}'"
            echo "curl -s http://127.0.0.1:10332 -H 'Content-Type: application/json' \\"
            echo "  -d '{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getblock\",\"params\":[1000,1]}'"
            ;;
        7)
            echo -e "${BLUE}🐳 Docker Demo:${NC}"
            echo ""
            echo "Docker commands:"
            echo "• docker build -t neo-node ."
            echo "• docker run -d --name neo-testnet -p 20332:20332 -p 20333:20333 -e NEO_NETWORK=testnet neo-node"
            echo "• docker run -d --name neo-mainnet -p 10332:10332 -p 10333:10333 -e NEO_NETWORK=mainnet neo-node"
            echo "• docker logs neo-testnet"
            echo "• docker stop neo-testnet neo-mainnet"
            ;;
        8)
            echo -e "${GREEN}👋 Goodbye!${NC}"
            exit 0
            ;;
        *)
            echo -e "${RED}❌ Invalid option. Please select 1-8.${NC}"
            ;;
    esac
done
