#!/bin/bash
# Neo-RS Node Health Check Script
# Monitors node status, sync progress, and system health

set -euo pipefail

RPC_URL="${RPC_URL:-http://localhost:10332}"
RPC_USER="${RPC_USER:-}"
RPC_PASS="${RPC_PASS:-}"
METRICS_URL="${METRICS_URL:-http://localhost:9090/metrics}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;36m'
NC='\033[0m'

check_service() {
    echo -e "${BLUE}=== Service Status ===${NC}"
    if systemctl is-active --quiet neo-node; then
        echo -e "${GREEN}✓${NC} neo-node service is running"
        systemctl status neo-node --no-pager -l | grep -E "(Active:|Memory:|CPU:)" || true
    else
        echo -e "${RED}✗${NC} neo-node service is not running"
        return 1
    fi
}

rpc_call() {
    local method="$1"
    local params="${2:-[]}"

    local auth_flag=""
    if [[ -n "$RPC_USER" && -n "$RPC_PASS" ]]; then
        auth_flag="-u $RPC_USER:$RPC_PASS"
    fi

    curl -sf $auth_flag "$RPC_URL" \
        -H "Content-Type: application/json" \
        -d "{\"jsonrpc\":\"2.0\",\"method\":\"$method\",\"params\":$params,\"id\":1}" \
        2>/dev/null | jq -r '.result' 2>/dev/null
}

check_rpc() {
    echo ""
    echo -e "${BLUE}=== RPC Endpoint ===${NC}"

    local version
    version=$(rpc_call "getversion")

    if [[ -n "$version" ]]; then
        echo -e "${GREEN}✓${NC} RPC endpoint responding"
        echo "  Version: $(echo "$version" | jq -r '.useragent' 2>/dev/null || echo "$version")"
        echo "  Network: $(echo "$version" | jq -r '.network' 2>/dev/null || echo "unknown")"
    else
        echo -e "${RED}✗${NC} RPC endpoint not responding"
        return 1
    fi
}

check_sync_status() {
    echo ""
    echo -e "${BLUE}=== Synchronization Status ===${NC}"

    local block_count
    block_count=$(rpc_call "getblockcount")

    if [[ -n "$block_count" && "$block_count" =~ ^[0-9]+$ ]]; then
        echo -e "${GREEN}✓${NC} Current block height: $block_count"

        # Try to get expected height from explorer API
        local expected_height
        expected_height=$(curl -sf https://api.neotube.io/api/mainnet/v1/get_height 2>/dev/null | jq -r '.height' 2>/dev/null || echo "")

        if [[ -n "$expected_height" && "$expected_height" =~ ^[0-9]+$ ]]; then
            local diff=$((expected_height - block_count))
            if [[ $diff -le 5 ]]; then
                echo -e "${GREEN}✓${NC} Node is synchronized (within $diff blocks)"
            elif [[ $diff -le 100 ]]; then
                echo -e "${YELLOW}⚠${NC} Node is catching up ($diff blocks behind)"
            else
                echo -e "${RED}✗${NC} Node is $diff blocks behind"
            fi
        fi
    else
        echo -e "${RED}✗${NC} Unable to get block count"
        return 1
    fi
}

check_peers() {
    echo ""
    echo -e "${BLUE}=== Peer Connections ===${NC}"

    local peer_count
    peer_count=$(rpc_call "getconnectioncount")

    if [[ -n "$peer_count" && "$peer_count" =~ ^[0-9]+$ ]]; then
        if [[ $peer_count -ge 10 ]]; then
            echo -e "${GREEN}✓${NC} Connected peers: $peer_count"
        elif [[ $peer_count -ge 3 ]]; then
            echo -e "${YELLOW}⚠${NC} Connected peers: $peer_count (low)"
        else
            echo -e "${RED}✗${NC} Connected peers: $peer_count (too low)"
        fi
    else
        echo -e "${YELLOW}⚠${NC} Unable to get peer count"
    fi
}

check_mempool() {
    echo ""
    echo -e "${BLUE}=== Mempool ===${NC}"

    local mempool
    mempool=$(rpc_call "getrawmempool")

    if [[ -n "$mempool" ]]; then
        local tx_count
        tx_count=$(echo "$mempool" | jq '. | length' 2>/dev/null || echo "0")
        echo -e "${GREEN}✓${NC} Mempool transactions: $tx_count"
    else
        echo -e "${YELLOW}⚠${NC} Unable to get mempool status"
    fi
}

check_system_resources() {
    echo ""
    echo -e "${BLUE}=== System Resources ===${NC}"

    # CPU usage
    local cpu_usage
    cpu_usage=$(top -bn1 | grep "neo-node" | awk '{print $9}' | head -1)
    if [[ -n "$cpu_usage" ]]; then
        echo "  CPU: ${cpu_usage}%"
    fi

    # Memory usage
    local mem_usage
    mem_usage=$(ps aux | grep neo-node | grep -v grep | awk '{sum+=$6} END {printf "%.1f", sum/1024/1024}')
    if [[ -n "$mem_usage" ]]; then
        echo "  Memory: ${mem_usage}GB"
    fi

    # Disk usage
    local disk_usage
    disk_usage=$(df -h /opt/neo-node/data 2>/dev/null | awk 'NR==2 {print $3 " / " $2 " (" $5 ")"}')
    if [[ -n "$disk_usage" ]]; then
        echo "  Disk: $disk_usage"
    fi

    # Check disk space warning
    local disk_percent
    disk_percent=$(df /opt/neo-node/data 2>/dev/null | awk 'NR==2 {print $5}' | sed 's/%//')
    if [[ -n "$disk_percent" && $disk_percent -gt 90 ]]; then
        echo -e "${RED}✗${NC} WARNING: Disk usage above 90%"
    fi
}

check_metrics() {
    echo ""
    echo -e "${BLUE}=== Metrics Endpoint ===${NC}"

    if curl -sf "$METRICS_URL" > /dev/null 2>&1; then
        echo -e "${GREEN}✓${NC} Prometheus metrics available at $METRICS_URL"

        # Sample some key metrics
        local metrics
        metrics=$(curl -sf "$METRICS_URL")

        echo ""
        echo "Key Metrics:"
        echo "$metrics" | grep "^neo_blockchain_height" | head -1 || true
        echo "$metrics" | grep "^neo_p2p_peer_count" | head -1 || true
        echo "$metrics" | grep "^neo_rpc_requests_total" | head -1 || true
    else
        echo -e "${YELLOW}⚠${NC} Metrics endpoint not accessible"
    fi
}

check_logs() {
    echo ""
    echo -e "${BLUE}=== Recent Logs ===${NC}"
    echo "Last 10 log entries:"
    journalctl -u neo-node -n 10 --no-pager | tail -10 || true

    # Check for errors
    local error_count
    error_count=$(journalctl -u neo-node --since "1 hour ago" | grep -i "error" | wc -l)
    if [[ $error_count -gt 0 ]]; then
        echo -e "${YELLOW}⚠${NC} Found $error_count errors in last hour"
    fi
}

summary() {
    echo ""
    echo -e "${BLUE}================================================${NC}"
    echo -e "${BLUE}Health Check Complete${NC}"
    echo -e "${BLUE}================================================${NC}"
    echo "Timestamp: $(date)"
}

main() {
    echo -e "${BLUE}================================================${NC}"
    echo -e "${BLUE}Neo-RS Node Health Check${NC}"
    echo -e "${BLUE}================================================${NC}"

    check_service || true
    check_rpc || true
    check_sync_status || true
    check_peers || true
    check_mempool || true
    check_system_resources || true
    check_metrics || true
    check_logs || true
    summary
}

main "$@"
