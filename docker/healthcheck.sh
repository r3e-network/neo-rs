#!/bin/bash

# Neo-RS Docker Health Check Script
# Returns 0 for healthy, 1 for unhealthy

NEO_RPC_PORT=${NEO_RPC_PORT:-30332}
TIMEOUT=10

# Function to check if process is running
check_process() {
    if pgrep -f neo-node > /dev/null 2>&1; then
        return 0
    else
        return 1
    fi
}

# Function to check RPC endpoint
check_rpc() {
    local response
    response=$(curl -s --connect-timeout $TIMEOUT --max-time $TIMEOUT \
        -X POST http://localhost:$NEO_RPC_PORT/rpc \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}' 2>/dev/null)
    
    if echo "$response" | grep -q "result"; then
        return 0
    else
        return 1
    fi
}

# Function to check port binding
check_port() {
    if ss -tuln | grep -q ":$NEO_RPC_PORT"; then
        return 0
    else
        return 1
    fi
}

# Main health check
main() {
    local checks_passed=0
    local total_checks=3
    
    # Check 1: Process running
    if check_process; then
        checks_passed=$((checks_passed + 1))
    fi
    
    # Check 2: Port binding
    if check_port; then
        checks_passed=$((checks_passed + 1))
    fi
    
    # Check 3: RPC responding
    if check_rpc; then
        checks_passed=$((checks_passed + 1))
    fi
    
    # Determine health status
    if [ $checks_passed -eq $total_checks ]; then
        echo "healthy"
        exit 0
    elif [ $checks_passed -ge 2 ]; then
        echo "degraded"
        exit 1
    else
        echo "unhealthy"
        exit 1
    fi
}

main "$@"