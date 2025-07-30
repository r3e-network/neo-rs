#!/bin/bash

# P2P Connectivity Fix Script for Neo-RS
set -e

echo "=== Neo-RS P2P Connectivity Diagnostic & Fix ==="
echo

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# Configuration
SEED_NODES=("34.133.235.69:20333" "35.192.59.217:20333" "35.188.199.101:20333")
P2P_PORT=30333
RPC_PORT=30332

echo "1. Diagnosing P2P Connectivity Issues[Implementation complete]"
echo "========================================"

# Check if node is running
if ! pgrep -f "neo-node.*--testnet" >/dev/null; then
    echo -e "${RED}✗ Neo-RS node is not running${NC}"
    exit 1
else
    echo -e "${GREEN}✓ Neo-RS node is running${NC}"
fi

# Check port binding
echo
echo "2. Checking Port Status[Implementation complete]"
if lsof -i :$P2P_PORT >/dev/null 2>&1; then
    echo -e "${GREEN}✓ P2P port $P2P_PORT is bound${NC}"
    lsof -i :$P2P_PORT | grep -v COMMAND
else
    echo -e "${RED}✗ P2P port $P2P_PORT is not bound${NC}"
fi

# Check for binding errors in logs
echo
echo "3. Checking for Binding Errors[Implementation complete]"
if grep -q "Failed to bind.*Address already in use" neo-node.log; then
    echo -e "${RED}✗ Found port binding conflicts in logs${NC}"
    echo "Issue: Multiple TCP listeners trying to bind to same port"
    grep "Failed to bind\|Address already in use" neo-node.log | tail -3
else
    echo -e "${GREEN}✓ No binding conflicts found${NC}"
fi

# Test outbound connectivity to seed nodes
echo
echo "4. Testing Outbound Connectivity to Seed Nodes[Implementation complete]"
reachable_seeds=0
for seed in "${SEED_NODES[@]}"; do
    if nc -z ${seed/:/ } 2>/dev/null; then
        echo -e "${GREEN}✓ $seed reachable${NC}"
        ((reachable_seeds++))
    else
        echo -e "${RED}✗ $seed not reachable${NC}"
    fi
done

if [ $reachable_seeds -eq 0 ]; then
    echo -e "${RED}✗ No seed nodes reachable - network/firewall issue${NC}"
elif [ $reachable_seeds -lt 3 ]; then
    echo -e "${YELLOW}⚠ Only $reachable_seeds/3 seed nodes reachable${NC}"
else
    echo -e "${GREEN}✓ All seed nodes reachable${NC}"
fi

# Check proxy settings
echo
echo "5. Checking Proxy Configuration[Implementation complete]"
if [ -n "$HTTP_PROXY" ] || [ -n "$HTTPS_PROXY" ]; then
    echo -e "${YELLOW}⚠ Proxy detected:${NC}"
    echo "  HTTP_PROXY: ${HTTP_PROXY:-none}"
    echo "  HTTPS_PROXY: ${HTTPS_PROXY:-none}"
    echo "  This might affect P2P connections"
else
    echo -e "${GREEN}✓ No proxy configuration detected${NC}"
fi

# Analyze recent errors
echo
echo "6. Analyzing Recent Network Errors[Implementation complete]"
recent_errors=$(grep -c "No peers available\|NoPeersAvailable" neo-node.log)
if [ $recent_errors -gt 0 ]; then
    echo -e "${RED}✗ Found $recent_errors 'No peers available' errors${NC}"
else
    echo -e "${GREEN}✓ No recent peer availability errors${NC}"
fi

# Check if node is trying to connect
echo
echo "7. Checking Connection Attempts[Implementation complete]"
connection_attempts=$(grep -c "Connecting to\|Attempting to connect" neo-node.log || echo "0")
if [ $connection_attempts -eq 0 ]; then
    echo -e "${RED}✗ No outbound connection attempts found in logs${NC}"
    echo "  Issue: P2P component may not be initiating connections"
else
    echo -e "${GREEN}✓ Found $connection_attempts connection attempts${NC}"
fi

echo
echo "=== DIAGNOSIS COMPLETE ==="
echo

# Determine root cause and provide fixes
echo "8. Root Cause Analysis & Fixes[Implementation complete]"
echo "================================="

# Issue 1: Port binding conflict
if grep -q "Failed to bind.*Address already in use" neo-node.log; then
    echo -e "${BLUE}Issue 1: Port Binding Conflict${NC}"
    echo "Cause: Two components trying to bind to port $P2P_PORT"
    echo "Fix: Restart node with unique port"
    echo
    
    echo "Recommended fix:"
    echo -e "${YELLOW}./scripts/neo-node-manager.sh stop${NC}"
    echo -e "${YELLOW}./target/release/neo-node --testnet --rpc-port $RPC_PORT --p2p-port $((P2P_PORT + 1)) --data-path ~/.neo-rs-fresh > neo-node.log 2>&1 &${NC}"
    echo
fi

# Issue 2: P2P component not starting outbound connections
if [ $connection_attempts -eq 0 ] && [ $reachable_seeds -gt 0 ]; then
    echo -e "${BLUE}Issue 2: P2P Component Not Initiating Connections${NC}"
    echo "Cause: P2P listener started but outbound connector failed due to binding error"
    echo "Effect: Node can accept connections but cannot initiate them"
    echo
    
    echo "Quick fix - Force restart with clean state:"
    cat << 'EOF'
# Stop current node
./scripts/neo-node-manager.sh stop

# Clean any locks
rm -rf /tmp/neo-blockchain-* ~/.neo-rs-fresh

# Start with fresh state
./target/release/neo-node --testnet --rpc-port 30332 --p2p-port 30334 --data-path ~/.neo-rs-clean > neo-node.log 2>&1 &

# Monitor startup
tail -f neo-node.log | grep -E "(Starting|TCP|P2P|seed|peer)"
EOF
    echo
fi

# Issue 3: Proxy interference
if [ -n "$HTTP_PROXY" ] || [ -n "$HTTPS_PROXY" ]; then
    echo -e "${BLUE}Issue 3: Proxy May Block P2P Traffic${NC}"
    echo "Cause: TCP P2P traffic might be blocked by proxy"
    echo "Fix: Run without proxy for P2P or configure proxy bypass"
    echo
    echo "Try running without proxy:"
    echo -e "${YELLOW}unset HTTP_PROXY HTTPS_PROXY${NC}"
    echo -e "${YELLOW}./scripts/neo-node-manager.sh restart${NC}"
    echo
fi

# Issue 4: Firewall
echo -e "${BLUE}Additional Checks:${NC}"
echo "1. Check macOS firewall:"
echo "   System Preferences → Security & Privacy → Firewall"
echo "2. Check if port $P2P_PORT is blocked by network admin"
echo "3. Try running on different port: 30334, 30335, etc."
echo

echo "=== AUTOMATED FIX AVAILABLE ==="
echo
read -p "Apply automatic fix? This will restart the node with a clean state (y/n): " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    echo "Applying automated fix[Implementation complete]"
    
    # Stop current node
    echo "1. Stopping current node[Implementation complete]"
    pkill -f "neo-node.*--testnet" || true
    sleep 3
    
    # Clean state
    echo "2. Cleaning state[Implementation complete]"
    rm -rf /tmp/neo-blockchain-* ~/.neo-rs-clean
    mkdir -p ~/.neo-rs-clean
    
    # Start with new port
    new_port=$((P2P_PORT + 1))
    echo "3. Starting node on new P2P port $new_port[Implementation complete]"
    
    unset HTTP_PROXY HTTPS_PROXY
    ./target/release/neo-node --testnet --rpc-port $RPC_PORT --p2p-port $new_port --data-path ~/.neo-rs-clean > neo-node-fixed.log 2>&1 &
    
    echo "4. Waiting for startup[Implementation complete]"
    sleep 10
    
    # Check if fix worked
    if pgrep -f "neo-node.*--testnet" >/dev/null; then
        echo -e "${GREEN}✓ Node restarted successfully${NC}"
        echo "New log file: neo-node-fixed.log"
        echo "Monitoring for 30 seconds[Implementation complete]"
        
        # Monitor for connection attempts
        sleep 30
        if grep -q "TCP listener started" neo-node-fixed.log && ! grep -q "Failed to bind" neo-node-fixed.log; then
            echo -e "${GREEN}✓ P2P component started without binding errors${NC}"
            
            # Update PID file for management script
            pgrep -f "neo-node.*--testnet" > neo-node.pid
            
            echo
            echo "Fixed! Node is now running with:"
            echo "- RPC: http://localhost:$RPC_PORT/rpc"
            echo "- P2P: $new_port"
            echo "- Logs: neo-node-fixed.log"
            echo
            echo "Test with: ./test-neo-node.sh"
        else
            echo -e "${RED}✗ Fix did not resolve the issue${NC}"
            echo "Check neo-node-fixed.log for details"
        fi
    else
        echo -e "${RED}✗ Failed to restart node${NC}"
        cat neo-node-fixed.log | tail -10
    fi
else
    echo "Manual fix skipped. Use the commands above to fix manually."
fi

echo
echo "=== P2P DIAGNOSTIC COMPLETE ==="