#!/bin/bash

# Neo-Node Manager Script
# Helps manage the Neo-Rust node without Docker

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
BINARY_PATH="$PROJECT_ROOT/target/release/neo-node"
PID_FILE="$PROJECT_ROOT/neo-node.pid"
LOG_FILE="$PROJECT_ROOT/neo-node.log"

# Default ports (can be overridden)
RPC_PORT=${NEO_RPC_PORT:-30332}
P2P_PORT=${NEO_P2P_PORT:-30333}

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

print_usage() {
    echo "Usage: $0 {start|stop|restart|status|logs|test}"
    echo ""
    echo "Commands:"
    echo "  start    - Start the Neo node"
    echo "  stop     - Stop the Neo node"
    echo "  restart  - Restart the Neo node"
    echo "  status   - Check node status"
    echo "  logs     - View node logs"
    echo "  test     - Test RPC endpoint"
    echo ""
    echo "Environment variables:"
    echo "  NEO_RPC_PORT - RPC port (default: 30332)"
    echo "  NEO_P2P_PORT - P2P port (default: 30333)"
}

build_if_needed() {
    if [ ! -f "$BINARY_PATH" ]; then
        echo -e "${YELLOW}Binary not found. Building[Implementation complete]${NC}"
        cd "$PROJECT_ROOT"
        cargo build --release --bin neo-node
        if [ $? -ne 0 ]; then
            echo -e "${RED}Build failed${NC}"
            exit 1
        fi
    fi
}

start_node() {
    if [ -f "$PID_FILE" ]; then
        PID=$(cat "$PID_FILE")
        if ps -p $PID > /dev/null 2>&1; then
            echo -e "${YELLOW}Node is already running (PID: $PID)${NC}"
            return
        fi
    fi

    build_if_needed

    echo -e "${GREEN}Starting Neo node[Implementation complete]${NC}"
    echo "RPC Port: $RPC_PORT"
    echo "P2P Port: $P2P_PORT"
    
    nohup "$BINARY_PATH" --testnet --rpc-port $RPC_PORT --p2p-port $P2P_PORT > "$LOG_FILE" 2>&1 &
    PID=$!
    echo $PID > "$PID_FILE"
    
    sleep 2
    
    if ps -p $PID > /dev/null; then
        echo -e "${GREEN}Node started successfully (PID: $PID)${NC}"
        echo "Logs: tail -f $LOG_FILE"
    else
        echo -e "${RED}Failed to start node${NC}"
        rm -f "$PID_FILE"
        exit 1
    fi
}

stop_node() {
    if [ ! -f "$PID_FILE" ]; then
        echo -e "${YELLOW}PID file not found. Checking for running processes[Implementation complete]${NC}"
        PIDS=$(pgrep -f "neo-node.*--testnet")
        if [ -n "$PIDS" ]; then
            echo -e "${YELLOW}Found neo-node process(es): $PIDS${NC}"
            kill $PIDS
            echo -e "${GREEN}Stopped neo-node process(es)${NC}"
        else
            echo -e "${YELLOW}No neo-node process found${NC}"
        fi
        return
    fi
    
    PID=$(cat "$PID_FILE")
    if ps -p $PID > /dev/null 2>&1; then
        echo -e "${GREEN}Stopping node (PID: $PID)[Implementation complete]${NC}"
        kill $PID
        sleep 2
        
        if ps -p $PID > /dev/null 2>&1; then
            echo -e "${YELLOW}Node still running, forcing stop[Implementation complete]${NC}"
            kill -9 $PID
        fi
        
        rm -f "$PID_FILE"
        echo -e "${GREEN}Node stopped${NC}"
    else
        echo -e "${YELLOW}Node not running${NC}"
        rm -f "$PID_FILE"
    fi
}

check_status() {
    if [ -f "$PID_FILE" ]; then
        PID=$(cat "$PID_FILE")
        if ps -p $PID > /dev/null 2>&1; then
            echo -e "${GREEN}Node is running (PID: $PID)${NC}"
            echo ""
            echo "Process info:"
            ps -p $PID -o pid,vsz,rss,pcpu,pmem,etime,args
        else
            echo -e "${RED}Node is not running (stale PID file)${NC}"
            rm -f "$PID_FILE"
        fi
    else
        PIDS=$(pgrep -f "neo-node.*--testnet")
        if [ -n "$PIDS" ]; then
            echo -e "${YELLOW}Found neo-node process(es) not managed by this script: $PIDS${NC}"
            ps -p $PIDS -o pid,vsz,rss,pcpu,pmem,etime,args
        else
            echo -e "${YELLOW}Node is not running${NC}"
        fi
    fi
    
    echo ""
    echo "RPC endpoint: http://localhost:$RPC_PORT/rpc"
    echo "P2P port: $P2P_PORT"
}

view_logs() {
    if [ -f "$LOG_FILE" ]; then
        echo -e "${GREEN}Viewing logs (press Ctrl+C to exit)[Implementation complete]${NC}"
        tail -f "$LOG_FILE"
    else
        echo -e "${YELLOW}Log file not found${NC}"
    fi
}

test_rpc() {
    echo -e "${GREEN}Testing RPC endpoint[Implementation complete]${NC}"
    echo ""
    
    echo "1. Getting version:"
    curl -s -X POST http://localhost:$RPC_PORT/rpc \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}' | jq .
    
    echo ""
    echo "2. Getting block count:"
    curl -s -X POST http://localhost:$RPC_PORT/rpc \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","method":"getblockcount","params":[],"id":1}' | jq .
    
    echo ""
    echo "3. Getting best block hash:"
    curl -s -X POST http://localhost:$RPC_PORT/rpc \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","method":"getbestblockhash","params":[],"id":1}' | jq .
}

case "$1" in
    start)
        start_node
        ;;
    stop)
        stop_node
        ;;
    restart)
        stop_node
        sleep 1
        start_node
        ;;
    status)
        check_status
        ;;
    logs)
        view_logs
        ;;
    test)
        test_rpc
        ;;
    *)
        print_usage
        exit 1
        ;;
esac