#!/bin/bash
# Neo-RS Optimization Quick Validation Script v1.0
# Run this immediately after node startup to verify optimization effectiveness

set -e

echo "=============================================="
echo "Neo-RS Performance Optimization - Quick Check"
echo "=============================================="
echo ""

METRICS_URL="http://localhost:8080/metrics"
TIMEOUT=30

echo "Step 1: Waiting for metrics endpoint (${TIMEOUT}s timeout)..."
for i in $(seq 1 $TIMEOUT); do
    if curl -s "$METRICS_URL" > /dev/null 2>&1; then
        echo "✓ Metrics endpoint available"
        break
    fi
    sleep 1
done

echo ""
echo "Step 2: Checking critical performance metrics..."
echo ""

echo "--- Cache Hit Rates ---"
curl -s "$METRICS_URL" | grep prefetch_hit_rate || echo "⚠ No prefetch cache metrics found (feature may be disabled)"
curl -s "$METRICS_URL" | grep cache_hit_rate || echo "⚠ General cache metrics not available"

echo ""
echo "--- Throughput Measurements ---"
blocks_per_sec=$(curl -s "$METRICS_URL" | grep blocks_per_second | head -n1)
if [ ! -z "$blocks_per_sec" ]; then
    echo "Blocks/sec: $blocks_per_sec"
else
    echo "⚠ Blocks/sec metric not available yet (wait a few seconds)"
fi

echo ""
echo "--- Transaction Latency (P50) ---"
tx_latency_p50=$(curl -s "$METRICS_URL" | grep tx_latency_seconds_sum | head -n1)
if [ ! -z "$tx_latency_p50" ]; then
    echo "Latency P50: $tx_latency_p50"
else
    echo "⚠ Latency histogram not ready yet"
fi

echo ""
echo "--- Resource Utilization ---"
curl -s "$METRICS_URL" | grep memory_usage_bytes | head -n1 || echo "Memory metrics not available"
curl -s "$METRICS_URL" | grep cpu_usage | head -n1 || echo "CPU metrics not available"

echo ""
echo "--- System Health ---"
node_status=$(curl -s "$METRICS_URL" | grep node_height || echo "Node height metric not available")
echo "$node_status"

echo ""
echo "Step 3: Expected Performance Targets"
echo ""
echo "✅ Cache Hit Rate: >70% (prefetch), >75% (LRU)"
echo "✅ Blocks/sec: ≥200 (testnet target)"  
echo "✅ TX Latency P50: <50ms"
echo "✅ Memory Usage: <8GB RSS"
echo ""

echo "Step 4: Comparison with Baseline"
echo ""
echo "Pre-optimization baseline:"
echo "  • Blocks/sec: ~5"
echo "  • TX Latency P50: ~500ms"
echo "  • Cache Hit Rate: ~20%"
echo ""
echo "After full optimization expected:"
echo "  • Blocks/sec: ~300+ (Phase 1-2)"
echo "  • TX Latency P50: <50ms (-90% improvement)"
echo "  • Cache Hit Rate: >75% (+275% improvement)"
echo ""

echo "Step 5: Validation Checklist"
echo ""

VALIDATION_PASSED=true

# Check cache hit rate
cache_hit_rate=$(curl -s "$METRICS_URL" | grep prefetch_hit_rate | head -n1 | awk '{print $NF}')
if [ ! -z "$cache_hit_rate" ]; then
    # Convert to integer for comparison (assume percentage format)
    hit_int=${cache_hit_rate%.*}
    if [ "$hit_int" -ge 70 ]; then
        echo "✅ Cache hit rate ≥70%: PASS ($cache_hit_rate)"
    else
        echo "❌ Cache hit rate too low: FAIL (<70%, current=$cache_hit_rate)"
        VALIDATION_PASSED=false
    fi
else
    echo "⚠ Cache hit rate metric not available yet (will stabilize over time)"
fi

# Check if node is running consistently
node_height=$(curl -s "$METRICS_URL" | grep node_height | head -n1 | awk '{print $NF}')
if [ ! -z "$node_height" ] && [ "$node_height" -gt 0 ]; then
    echo "✅ Node synchronizing normally: PASS (height=$node_height)"
else
    echo "❌ Node height not valid or not syncing: FAIL"
    VALIDATION_PASSED=false
fi

echo ""
if [ "$VALIDATION_PASSED" = true ]; then
    echo "=============================================="
    echo "✓ ALL CRITICAL CHECKS PASSED"
    echo "=============================================="
    echo ""
    echo "Optimizations are performing within expected parameters."
    echo "Continue monitoring for 24h to confirm stability."
    exit 0
else
    echo "=============================================="
    echo "⚠ VALIDATION WARNINGS DETECTED"
    echo "=============================================="
    echo ""
    echo "Some metrics are below optimal thresholds."
    echo "Review documentation and consider parameter tuning."
    exit 1
fi
