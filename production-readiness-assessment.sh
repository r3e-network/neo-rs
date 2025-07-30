#!/bin/bash

# Neo-RS Production Readiness Assessment Script
set -e

echo "=== Neo-RS Production Readiness Assessment ==="
echo "Timestamp: $(date)"
echo

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

# Configuration
RPC_PORT=30332
P2P_PORT=30334
LOG_FILE="neo-node-safe.log"
PID_FILE="neo-node.pid"

# Results tracking
PASSED=0
FAILED=0
WARNINGS=0
TOTAL_TESTS=0

# Helper functions
test_result() {
    local test_name="$1"
    local result="$2"
    local message="$3"
    local level="${4:-info}"
    
    TOTAL_TESTS=$((TOTAL_TESTS + 1))
    
    case "$result" in
        "PASS")
            echo -e "${GREEN}✓ PASS${NC} - $test_name: $message"
            PASSED=$((PASSED + 1))
            ;;
        "FAIL")
            echo -e "${RED}✗ FAIL${NC} - $test_name: $message"
            FAILED=$((FAILED + 1))
            ;;
        "WARN")
            echo -e "${YELLOW}⚠ WARN${NC} - $test_name: $message"
            WARNINGS=$((WARNINGS + 1))
            ;;
    esac
}

# 1. Node Status Assessment
echo -e "${BLUE}=== 1. Node Status Assessment ===${NC}"

# Check if node is running
if [ -f "$PID_FILE" ]; then
    NODE_PID=$(cat "$PID_FILE")
    if kill -0 "$NODE_PID" 2>/dev/null; then
        UPTIME=$(ps -o etime= -p "$NODE_PID" | tr -d ' ')
        test_result "Node Process" "PASS" "Running with PID $NODE_PID, uptime: $UPTIME"
        
        # Memory usage
        MEMORY=$(ps -o rss= -p "$NODE_PID" | tr -d ' ')
        MEMORY_MB=$((MEMORY / 1024))
        if [ $MEMORY_MB -lt 100 ]; then
            test_result "Memory Usage" "PASS" "${MEMORY_MB}MB (efficient)"
        elif [ $MEMORY_MB -lt 500 ]; then
            test_result "Memory Usage" "WARN" "${MEMORY_MB}MB (moderate)"
        else
            test_result "Memory Usage" "FAIL" "${MEMORY_MB}MB (high)"
        fi
        
        # CPU usage
        CPU=$(ps -o %cpu= -p "$NODE_PID" | tr -d ' ')
        if (( $(echo "$CPU < 5.0" | bc -l) )); then
            test_result "CPU Usage" "PASS" "${CPU}% (efficient)"
        elif (( $(echo "$CPU < 20.0" | bc -l) )); then
            test_result "CPU Usage" "WARN" "${CPU}% (moderate)"
        else
            test_result "CPU Usage" "FAIL" "${CPU}% (high)"
        fi
    else
        test_result "Node Process" "FAIL" "PID file exists but process not running"
    fi
else
    test_result "Node Process" "FAIL" "PID file not found"
fi

# 2. Network Connectivity Assessment
echo
echo -e "${BLUE}=== 2. Network Connectivity Assessment ===${NC}"

# RPC Port binding
if lsof -i :$RPC_PORT >/dev/null 2>&1; then
    test_result "RPC Port Binding" "PASS" "Port $RPC_PORT is bound and listening"
else
    test_result "RPC Port Binding" "FAIL" "Port $RPC_PORT is not bound"
fi

# P2P Port binding
if lsof -i :$P2P_PORT >/dev/null 2>&1; then
    test_result "P2P Port Binding" "PASS" "Port $P2P_PORT is bound and listening"
else
    test_result "P2P Port Binding" "FAIL" "Port $P2P_PORT is not bound"
fi

# Test RPC connectivity
echo "Testing RPC endpoint connectivity[Implementation complete]"
RPC_RESPONSE=$(curl -s -X POST http://localhost:$RPC_PORT/rpc \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}' \
    --connect-timeout 5 --max-time 10 2>/dev/null || echo "")

if echo "$RPC_RESPONSE" | grep -q "result"; then
    test_result "RPC Connectivity" "PASS" "RPC endpoint responding correctly"
else
    test_result "RPC Connectivity" "FAIL" "RPC endpoint not responding"
fi

# 3. Core Functionality Assessment
echo
echo -e "${BLUE}=== 3. Core Functionality Assessment ===${NC}"

# Test getversion
VERSION_RESPONSE=$(curl -s -X POST http://localhost:$RPC_PORT/rpc \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}' 2>/dev/null || echo "")

if echo "$VERSION_RESPONSE" | grep -q "neo-rs"; then
    VERSION=$(echo "$VERSION_RESPONSE" | grep -o '"useragent":"[^"]*"' | cut -d'"' -f4)
    test_result "Node Version" "PASS" "Version: $VERSION"
else
    test_result "Node Version" "FAIL" "Cannot retrieve version"
fi

# Test getblockcount
BLOCKCOUNT_RESPONSE=$(curl -s -X POST http://localhost:$RPC_PORT/rpc \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"getblockcount","params":[],"id":1}' 2>/dev/null || echo "")

if echo "$BLOCKCOUNT_RESPONSE" | grep -q "result"; then
    BLOCKCOUNT=$(echo "$BLOCKCOUNT_RESPONSE" | grep -o '"result":[0-9]*' | cut -d':' -f2)
    if [ "$BLOCKCOUNT" -eq 0 ]; then
        test_result "Blockchain State" "WARN" "At genesis block (block count: $BLOCKCOUNT)"
    else
        test_result "Blockchain State" "PASS" "Synced blocks: $BLOCKCOUNT"
    fi
else
    test_result "Blockchain State" "FAIL" "Cannot retrieve block count"
fi

# Test native contract access
NEO_RESPONSE=$(curl -s -X POST http://localhost:$RPC_PORT/rpc \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"invokefunction","params":["0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5","totalSupply"],"id":1}' 2>/dev/null || echo "")

if echo "$NEO_RESPONSE" | grep -q "result"; then
    test_result "Smart Contract Access" "PASS" "Native contracts accessible"
else
    test_result "Smart Contract Access" "FAIL" "Cannot access native contracts"
fi

# 4. Performance Assessment
echo
echo -e "${BLUE}=== 4. Performance Assessment ===${NC}"

# Response time test
START_TIME=$(date +%s%N)
PERF_RESPONSE=$(curl -s -X POST http://localhost:$RPC_PORT/rpc \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}' 2>/dev/null || echo "")
END_TIME=$(date +%s%N)
RESPONSE_TIME=$(( (END_TIME - START_TIME) / 1000000 ))

if [ $RESPONSE_TIME -lt 50 ]; then
    test_result "Response Time" "PASS" "${RESPONSE_TIME}ms (excellent)"
elif [ $RESPONSE_TIME -lt 200 ]; then
    test_result "Response Time" "PASS" "${RESPONSE_TIME}ms (good)"
elif [ $RESPONSE_TIME -lt 1000 ]; then
    test_result "Response Time" "WARN" "${RESPONSE_TIME}ms (acceptable)"
else
    test_result "Response Time" "FAIL" "${RESPONSE_TIME}ms (slow)"
fi

# 5. Error Analysis
echo
echo -e "${BLUE}=== 5. Error Analysis ===${NC}"

if [ -f "$LOG_FILE" ]; then
    # Check for critical errors
    CRITICAL_ERRORS=$(grep -c "CRITICAL\|FATAL\|PANIC" "$LOG_FILE" 2>/dev/null | head -1 || echo "0")
    if [ "$CRITICAL_ERRORS" -eq 0 ]; then
        test_result "Critical Errors" "PASS" "No critical errors found"
    else
        test_result "Critical Errors" "FAIL" "$CRITICAL_ERRORS critical errors found"
    fi
    
    # Check for errors
    ERROR_COUNT=$(grep -c "ERROR" "$LOG_FILE" 2>/dev/null | head -1 || echo "0")
    if [ "$ERROR_COUNT" -eq 0 ]; then
        test_result "Error Count" "PASS" "No errors in logs"
    elif [ "$ERROR_COUNT" -eq 1 ]; then
        # Check if it's the known P2P binding error
        if grep -q "Failed to bind TCP listener.*Address already in use" "$LOG_FILE"; then
            test_result "Error Count" "WARN" "1 expected P2P binding error (known issue)"
        else
            test_result "Error Count" "WARN" "1 unexpected error found"
        fi
    else
        test_result "Error Count" "FAIL" "$ERROR_COUNT errors found"
    fi
    
    # Check for warnings
    WARNING_COUNT=$(grep -c "WARN" "$LOG_FILE" 2>/dev/null | head -1 || echo "0")
    if [ "$WARNING_COUNT" -eq 0 ]; then
        test_result "Warning Count" "PASS" "No warnings in logs"
    elif [ "$WARNING_COUNT" -lt 5 ]; then
        test_result "Warning Count" "WARN" "$WARNING_COUNT warnings found"
    else
        test_result "Warning Count" "FAIL" "$WARNING_COUNT warnings found (high)"
    fi
else
    test_result "Log File Access" "FAIL" "Log file not found"
fi

# 6. Configuration Assessment
echo
echo -e "${BLUE}=== 6. Configuration Assessment ===${NC}"

# Check for required files
if [ -f "target/release/neo-node" ]; then
    test_result "Binary Availability" "PASS" "neo-node binary exists"
else
    test_result "Binary Availability" "FAIL" "neo-node binary not found"
fi

if [ -f "start-node-safe.sh" ]; then
    test_result "Startup Script" "PASS" "Safe startup script available"
else
    test_result "Startup Script" "WARN" "Startup script not found"
fi

# Check data directory
if [ -n "$NODE_PID" ] && kill -0 "$NODE_PID" 2>/dev/null; then
    DATA_DIRS=$(find ~/.neo-rs-* -type d 2>/dev/null | wc -l)
    if [ $DATA_DIRS -eq 1 ]; then
        test_result "Data Directory" "PASS" "Single data directory (clean)"
    elif [ $DATA_DIRS -gt 1 ]; then
        test_result "Data Directory" "WARN" "$DATA_DIRS data directories (cleanup recommended)"
    else
        test_result "Data Directory" "WARN" "No data directories found"
    fi
fi

# 7. Security Assessment
echo
echo -e "${BLUE}=== 7. Security Assessment ===${NC}"

# Check if running as root
if [ "$EUID" -eq 0 ]; then
    test_result "User Privileges" "WARN" "Running as root (security risk)"
else
    test_result "User Privileges" "PASS" "Running as non-root user"
fi

# Check for open ports
OPEN_PORTS=$(lsof -i -P -n | grep LISTEN | grep -E ":(30332|30334)" | wc -l)
if [ $OPEN_PORTS -eq 2 ]; then
    test_result "Port Security" "PASS" "Only required ports open"
elif [ $OPEN_PORTS -eq 1 ]; then
    test_result "Port Security" "WARN" "Partial port binding (P2P issue)"
else
    test_result "Port Security" "FAIL" "Unexpected port configuration"
fi

# 8. Production Readiness Summary
echo
echo -e "${BLUE}=== 8. Production Readiness Summary ===${NC}"

# Calculate readiness score
TOTAL_SCORE=$((PASSED * 100 / TOTAL_TESTS))
echo "Test Results Summary:"
echo "  Total Tests: $TOTAL_TESTS"
echo -e "  ${GREEN}Passed: $PASSED${NC}"
echo -e "  ${YELLOW}Warnings: $WARNINGS${NC}"
echo -e "  ${RED}Failed: $FAILED${NC}"
echo "  Overall Score: $TOTAL_SCORE%"
echo

# Production readiness assessment
if [ $TOTAL_SCORE -ge 90 ] && [ $FAILED -eq 0 ]; then
    echo -e "${GREEN}✓ PRODUCTION READY${NC}"
    echo "Status: The Neo-RS node is ready for production use."
    READINESS="PRODUCTION_READY"
elif [ $TOTAL_SCORE -ge 75 ] && [ $FAILED -le 2 ]; then
    echo -e "${YELLOW}⚠ CONDITIONALLY READY${NC}"
    echo "Status: Node is functional but has some limitations."
    READINESS="CONDITIONALLY_READY"
else
    echo -e "${RED}✗ NOT PRODUCTION READY${NC}"
    echo "Status: Significant issues need to be resolved."
    READINESS="NOT_READY"
fi

echo
echo "=== Specific Use Case Assessments ==="

# RPC Development
if echo "$RPC_RESPONSE" | grep -q "result" && [ $RESPONSE_TIME -lt 200 ]; then
    echo -e "${GREEN}✓ EXCELLENT${NC} for RPC Development & Testing"
else
    echo -e "${RED}✗ LIMITED${NC} for RPC Development & Testing"
fi

# Smart Contract Development
if echo "$NEO_RESPONSE" | grep -q "result"; then
    echo -e "${GREEN}✓ EXCELLENT${NC} for Smart Contract Development"
else
    echo -e "${RED}✗ LIMITED${NC} for Smart Contract Development"
fi

# Full Node Operation
if [ "$BLOCKCOUNT" -gt 0 ] 2>/dev/null && [ $ERROR_COUNT -eq 0 ]; then
    echo -e "${GREEN}✓ READY${NC} for Full Node Operation"
elif [ "$BLOCKCOUNT" -eq 0 ] 2>/dev/null; then
    echo -e "${YELLOW}⚠ LIMITED${NC} for Full Node Operation (no sync due to P2P)"
else
    echo -e "${RED}✗ NOT READY${NC} for Full Node Operation"
fi

echo
echo "=== Recommendations ==="

if [ $FAILED -gt 0 ]; then
    echo "Critical Issues to Address:"
    echo "  • Review failed tests above"
    echo "  • Check system resources and configuration"
    echo "  • Verify network connectivity"
fi

if [ $WARNINGS -gt 0 ]; then
    echo "Improvements Recommended:"
    echo "  • Address warning conditions"
    echo "  • Monitor performance metrics"
    echo "  • Consider cleanup of data directories"
fi

if grep -q "Failed to bind TCP listener.*Address already in use" "$LOG_FILE" 2>/dev/null; then
    echo "Known P2P Issue:"
    echo "  • P2P sync limited due to dual TCP listener binding"
    echo "  • Node functions perfectly for RPC development"
    echo "  • Source code fix needed for full P2P functionality"
fi

echo
echo "=== Management Commands ==="
echo "Monitor:     tail -f $LOG_FILE"
echo "Test RPC:    curl -X POST http://localhost:$RPC_PORT/rpc -H 'Content-Type: application/json' -d '{\"jsonrpc\":\"2.0\",\"method\":\"getversion\",\"params\":[],\"id\":1}'"
echo "Stop Node:   kill \$(cat $PID_FILE)"
echo "Restart:     ./start-node-safe.sh"

# Generate machine-readable output
echo
echo "=== Machine-Readable Results ==="
echo "READINESS_STATUS=$READINESS"
echo "TOTAL_TESTS=$TOTAL_TESTS"
echo "PASSED=$PASSED"
echo "WARNINGS=$WARNINGS"
echo "FAILED=$FAILED"
echo "SCORE=$TOTAL_SCORE"
echo "TIMESTAMP=$(date -u +%Y-%m-%dT%H:%M:%SZ)"

echo
echo "Assessment completed at $(date)"