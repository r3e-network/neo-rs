#!/bin/bash
# Neo-N3 Monitoring Verification Script
# Quick sanity check for all monitoring components

set -e

echo "======================================"
echo "Neo-N3 Monitoring Verification"
echo "======================================"
echo ""

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

check_count=0
pass_count=0
fail_count=0

check_step() {
    check_count=$((check_count + 1))
    echo -n "[${check_count}] $1... "
}

success() {
    echo -e "${GREEN}✓${NC}"
    pass_count=$((pass_count + 1))
}

warning() {
    echo -e "${YELLOW}⚠${NC} ($1)"
    pass_count=$((pass_count + 1))
}

error() {
    echo -e "${RED}✗${NC} ($1)"
    fail_count=$((fail_count + 1))
}

# Test 1: Check if node metrics endpoint is accessible
check_step "Node metrics endpoint (port 9090)"
if curl -s http://127.0.0.1:9090/metrics > /dev/null 2>&1; then
    success
else
    error "Metrics endpoint not responding"
fi

# Test 2: Verify health endpoint returns JSON
check_step "Health endpoint (/healthz)"
HEALTH_RESPONSE=$(curl -s http://127.0.0.1:9090/healthz)
if [ $? -eq 0 ] && echo "$HEALTH_RESPONSE" | grep -q "status"; then
    success
    # Extract status
    STATUS=$(echo "$HEALTH_RESPONSE" | grep -o '"status"[[:space:]]*:[[:space:]]*"[^"]*"' | cut -d'"' -f4)
    echo "      Status: $STATUS"
else
    error "Health endpoint not returning valid JSON"
fi

# Test 3: Verify readiness endpoint
check_step "Readiness endpoint (/ready)"
READY_RESPONSE=$(curl -s http://127.0.0.1:9090/ready)
if [ $? -eq 0 ] && echo "$READY_RESPONSE" | grep -q "status"; then
    success
else
    error "Readiness endpoint not responding"
fi

# Test 4: Check for essential metrics
check_step "Essential blockchain metrics"
METRICS_OUTPUT=$(curl -s http://127.0.0.1:9090/metrics)

REQUIRED_METRICS=(
    "neo_sync_current_height"
    "neo_sync_tip_height"
    "neo_peer_count"
    "neo_header_lag"
)

missing_metrics=()
for metric in "${REQUIRED_METRICS[@]}"; do
    if ! echo "$METRICS_OUTPUT" | grep -q "$metric"; then
        missing_metrics+=("$metric")
    fi
done

if [ ${#missing_metrics[@]} -eq 0 ]; then
    success
else
    error "Missing metrics: ${missing_metrics[*]}"
fi

# Test 5: Check optimization metrics
check_step "Optimization metrics (Phase 1-3)"
OPT_METRICS=(
    "cuckoo_hash_lookups_total"
    "simdd_blake2b_throughput_bytes_per_second"
    "batch_signatures_verified_total"
    "prefetch_stage_latencies_bucket"
)

missing_opt=()
for metric in "${OPT_METRICS[@]}"; do
    if ! echo "$METRICS_OUTPUT" | grep -q "$metric"; then
        missing_opt+=("$metric")
    fi
done

if [ ${#missing_opt[@]} -eq 0 ]; then
    success
else
    warning "Some opt metrics missing: ${missing_opt[*]}"
fi

# Test 6: Parse health response for optimization status
check_step "Health endpoint optimization flags"
HAS_CUCKOO=$(echo "$HEALTH_RESPONSE" | grep -c "cuckoo_hash" || true)
HAS_BATCH=$(echo "$HEALTH_RESPONSE" | grep -c "batch_verification" || true)
HAS_SIMD=$(echo "$HEALTH_RESPONSE" | grep -c "simd_blake2b" || true)
HAS_PREFETCH=$(echo "$HEALTH_RESPONSE" | grep -c "prefetch_pipeline" || true)

if [ $HAS_CUCKOO -gt 0 ] && [ $HAS_BATCH -gt 0 ] && [ $HAS_SIMD -gt 0 ] && [ $HAS_PREFETCH -gt 0 ]; then
    success
else
    error "Missing optimization status flags in health response"
fi

# Test 7: Check sync progress metrics
check_step "Sync progress metrics"
if echo "$METRICS_OUTPUT" | grep -q "neo_fast_sync_enabled"; then
    FAST_SYNC=$(echo "$METRICS_OUTPUT" | grep "neo_fast_sync_enabled" | head -1 | awk '{print $NF}')
    echo "      Fast Sync Enabled: $FAST_SYNC"
    success
else
    warning "Fast sync flag not found (may not be active)"
fi

# Test 8: Verify CPU and memory metrics exist
check_step "System resource metrics"
HAS_CPU=$(echo "$METRICS_OUTPUT" | grep -c "cpu_usage_ratio" || true)
HAS_MEM=$(echo "$METRICS_OUTPUT" | grep -c "memory_usage_bytes" || true)

if [ $HAS_CPU -gt 0 ] && [ $HAS_MEM -gt 0 ]; then
    success
else
    error "Missing system resource metrics"
fi

# Test 9: Parse block height from metrics
check_step "Extract current block height"
BLOCK_HEIGHT=$(echo "$METRICS_OUTPUT" | grep "neo_sync_current_height" | head -1 | awk '{print $NF}')
if [ -n "$BLOCK_HEIGHT" ] && [ "$BLOCK_HEIGHT" != "0" ]; then
    echo "      Current Block Height: $BLOCK_HEIGHT"
    success
else
    warning "Block height not set or at genesis"
fi

# Test 10: Extract peer count
check_step "Extract peer connection count"
PEER_COUNT=$(echo "$METRICS_OUTPUT" | grep "neo_peer_count" | head -1 | awk '{print $NF}')
if [ -n "$PEER_COUNT" ]; then
    echo "      Connected Peers: $PEER_COUNT"
    if [ "$PEER_COUNT" -ge 25 ]; then
        success
    else
        warning "Peer count below recommended minimum (25)"
    fi
else
    error "Could not extract peer count"
fi

# Summary
echo ""
echo "======================================"
echo "Verification Summary"
echo "======================================"
echo -e "Total Checks: ${check_count}"
echo -e "${GREEN}Passed: ${pass_count}${NC}"
echo -e "${RED}Failed: ${fail_count}${NC}"
echo ""

if [ $fail_count -eq 0 ]; then
    echo -e "${GREEN}All critical checks passed!${NC}"
    echo ""
    echo "Next steps:"
    echo "1. Review Grafana dashboard at http://localhost:3000"
    echo "2. Configure alerts based on your environment"
    echo "3. Set up notification channels (email, Slack)"
    echo "4. See docs/MONITORING.md for detailed operations guide"
    exit 0
else
    echo -e "${RED}Some checks failed. Please review the errors above.${NC}"
    echo ""
    echo "For troubleshooting, see docs/MONITORING.md"
    exit 1
fi
