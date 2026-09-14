# Neo-RS Performance Optimization: Quick Deployment & Validation Guide v1.0

## Purpose
Fast-track deployment and validation of all Tier 1-3 performance optimizations on testnet infrastructure.

---

## Phase 1: Pre-Deployment Checklist ✅

### Documentation Review
Review the following documents first:
- [ ] [`COMPREHENSIVE-OPTIMIZATION-SUMMARY.md`](./COMPREHENSIVE-OPTIMIZATION-SUMMARY.md) - Full technical report
- [ ] [`TIER1-OPTIMIZATION-SUMMARY.md`](./TIER1-OPTIMIZATION-SUMMARY.md) - Foundation optimizations
- [ ] [`ACCOUNT_PREFETCH_CACHE_IMPLEMENTATION_SUMMARY.md`](./ACCOUNT_PREFETCH_CACHE_IMPLEMENTATION_SUMMARY.md) - Prefetch details
- [ ] [`IMPLEMENTATION-SUMMARY.md`](./IMPLEMENTATION-SUMMARY.md) - VM execution engine optimizations

### Environment Setup
```bash
# Export optimization enablement variables
export ENABLE_ACCOUNT_PREFETCH=1
export MAX_PREFETCH_CACHE_SIZE=1000
export PREFETCH_PARALLELISM=4
export RUST_LOG="info,prefetch=debug,state_service=trace"

# Optional: Enable Prometheus metrics
export PROMETHEUS_PORT=8080
```

### Backup Current Configuration
```bash
cp neo-testnet-node.toml neo-testnet-node.toml.backup.original
git checkout HEAD -- neo-testnet-node.toml
```

---

## Phase 2: Build Optimization Branch 🔨

### Create Dedicated Branch
```bash
git checkout -b perf-optimization-tier3
git push origin perf-optimization-tier3
```

### Verify Dependencies
Ensure these crates are available in `Cargo.toml`:
- `crossbeam-channel = "0.5"` (for prefetch pipeline)
- `rayon = "1.7"` (for parallel workers)
- `bumpalo = "3.13"` (for arena allocation)
- `lru = "0.11"` (for LRU cache)
- `parking_lot = "0.12"` (for concurrent locks)

### Feature Flags
Build with specific features enabled:
```bash
# Basic optimizations only
cargo build --release --features "prefetch,runtime"

# All optimizations including TEE/HSM support  
cargo build --all-features --release
```

---

## Phase 3: Testnet Configuration ⚙️

### Update `neo-testnet-node.toml`

Add these sections to your configuration file:

```toml
# Existing Node settings remain unchanged
[Node]
max_connections = 100
relay = true

# Storage backend (RocksDB already default)
[Storage]
type = "rocksdb"
path = "./data/testnet"

# NEW: State service optimization settings
[StateService]
enable_prefetch_cache = true
max_prefetch_cache_size = 1000
prefetch_parallelism = 4

# Consensus parameters (adjust based on testnet requirements)
[Consensus]
block_time = 10000
validators = [...]
magic_number = 1692510522  # Testnet magic

# RPC settings with metrics endpoint
[Rpc]
enabled = true
port = 30333
metrics_port = 8080
cors_domains = ["*"]
```

### Save Configuration
```bash
cp neo-testnet-node.toml neo-testnet-node.toml.optimized
echo "Configuration saved as neo-testnet-node.toml.optimized"
```

---

## Phase 4: Start Optimized Node 🚀

### Launch Node with Logging
```bash
cd d:\Git\neo-rs

# Clean previous builds (optional, for fresh compilation)
cargo clean

# Run optimized binary with feature flags
cargo run --release --features "prefetch,runtime"
```

### Expected Output
Watch for initialization messages:
```
INFO  Starting Neo-RS node...
INFO  Loading MPT state tree from snapshot...
INFO  GlobalNodeCache initialized with capacity: 1M entries
INFO  AccountPrefetchCache enabled with parallel loading
INFO  Starting verification worker pool: 7 threads
INFO  RocksDB opened at path: ./data/testnet
INFO  Genesis block loaded successfully
INFO  P2P server started on port: 10333
INFO  RPC server started on port: 30333
INFO  Metrics server started on port: 8080
```

If you see errors related to prefetch features, verify the feature flag is correctly set:
```bash
cargo run --help | grep prefetch
```

---

## Phase 5: Performance Metrics Monitoring 📊

### Prometheus Endpoint

Access live metrics at:
```
http://localhost:8080/metrics
```

### Query Specific Metrics

#### 1. Check Cache Hit Rate (Target: >75%)
```bash
curl http://localhost:8080/metrics | grep prefetch_hit_rate
```

Expected output example:
```
prefetch_hit_rate{cache="account"} 0.78234
prefetch_hit_rate{cache="mpt_node"} 0.82145
```

#### 2. Monitor Blocks per Second (Target: 200-300 after T3)
```bash
curl http://localhost:8080/metrics | grep blocks_per_second
```

Example:
```
blocks_per_second{height="1234567"} 245.6
```

#### 3. Track Transaction Latency Distribution
```bash
curl http://localhost:8080/metrics | grep tx_latency_seconds
```

Key histogram buckets:
- P50 latency: `< 50ms` target
- P99 latency: `< 200ms` target

#### 4. Verify RocksDB Efficiency
```bash
curl http://localhost:8080/metrics | grep rocksdb_cache_used_bytes
```

Expected improvement: ~60% reduction compared to baseline

### Real-Time Monitoring Script
Save this as `monitor_metrics.sh`:
```bash
#!/bin/bash
while true; do
    clear
    echo "Neo-RS Performance Metrics - $(date)"
    echo "======================================"
    echo ""
    
    echo "Blocks/Second:"
    curl -s http://localhost:8080/metrics | grep blocks_per_second | head -n1
    
    echo ""
    echo "Cache Hit Rates:"
    curl -s http://localhost:8080/metrics | grep prefetch_hit_rate
    
    echo ""
    echo "Transaction Count:"
    curl -s http://localhost:8080/metrics | grep transactions_processed_total
    
    sleep 5
done
```

Make executable and run:
```bash
chmod +x monitor_metrics.sh
./monitor_metrics.sh
```

---

## Phase 6: Load Testing Suite 🧪

### Run Unit Tests
```bash
# Core optimizations tests
cargo test -p neo-crypto state_service::global_node_cache::tests
cargo test -p neo-core state_service::account_prefetcher::tests

# Memory pool batcher tests
cargo test -p neo-mempool batcher::contract_batcher::tests

# VM execution tests
cargo test -p neo-vm memory::arena_pool::tests
```

Expected: All tests passing (≥90% success rate)

### Run Benchmarks

#### Account Prefetch Benchmark
```bash
cd neo-core/benches
cargo bench --bench account_prefetch_bench
```

Expected result: **-10-15% latency reduction** vs non-prefetch version

#### Contract Batcher Benchmark
```bash
cargo bench --bench contract_batcher_bench
```

Expected result: **+20-30% TPS increase** over random ordering

#### Arena Memory Pool Benchmark
```bash
cargo bench --bench arena_memory_pool_bench
```

Expected result: **-96% heap allocations**, near-zero GC pressure

### Integration Tests
```bash
# Run full integration suite
./scripts/run_all_tests.sh --features prefetch

# Or individually
./tests/run_integration_tests.sh --test-category "mempool"
./tests/run_integration_tests.sh --test-category "state_service"
```

---

## Phase 7: Validation Checklist ✓

After running for ≥1 hour with real network traffic, verify these metrics:

### Cache Performance
- [ ] Prefetch cache hit rate >70%
- [ ] MPT node cache hit rate >75%  
- [ ] No OOM (Out-of-Memory) events

### Throughput Improvements
- [ ] Blocks/sec increased by ≥150%
- [ ] TX processing latency reduced by ≥15%
- [ ] Peak throughput exceeds baseline by ≥20%

### Resource Utilization
- [ ] Memory usage stable (<8GB RSS)
- [ ] CPU utilization ≥80% during peak load
- [ ] RocksDB I/O wait <20%

### Stability Indicators
- [ ] Zero crashes or panics
- [ ] No unhandled exceptions
- [ ] No consensus failures
- [ ] Peer connections stable

---

## Troubleshooting Guide 🛠️

### Issue: Cache Misses Too High (>50% miss rate)

**Symptoms**: Low prefetch effectiveness, continued high RocksDB I/O

**Solution**: Increase cache capacity
```bash
# Edit configuration
nano neo-testnet-node.toml
# Change:
max_prefetch_cache_size = 2000  # Double original value
prefetch_parallelism = 6        # Increase parallel loading threads
```

### Issue: Memory Pressure Detected (High RAM Usage)

**Symptoms**: System swapping, degraded performance

**Solution**: Reduce prefetch counts
```bash
# Temporarily disable prefetch for testing
export ENABLE_ACCOUNT_PREFETCH=0

# Or reduce cache size
max_prefetch_cache_size = 500
```

### Issue: RocksDB Slow I/O (Disk Latency Spikes)

**Symptoms**: I/O wait time >50%, cache thrashing

**Solution**: Upgrade storage or optimize RocksDB settings
```toml
# Add to neo-testnet-node.toml
[Storage.RocksDB]
block_size = 65536          # 64KB blocks
cache_size = 536870912      # 512MB cache
compaction_priority = "ioprofile"
```

### Issue: Prefetch Parallelism Underutilized (Low CPU Usage)

**Symptoms**: Workers idle, throughput below expected

**Solution**: Tune parallelism based on core count
```bash
# Detect available cores
nproc --all  # Linux
[System.Environment]::ProcessorCount  # PowerShell

# Adjust config accordingly
prefetch_parallelism = 4  # Example: 4 cores
```

### Issue: Contract Batching Inefficient (Small Batch Sizes)

**Symptoms**: Average batch size <10 transactions

**Solution**: Increase gas limit per batch
```toml
[ Mempool.Batcher ]
max_batch_gas = 80_000_000   # Increase from 50M default
max_batch_size = 150         # Increase transaction limit
```

---

## Rollback Plan 🔄

If any issues arise during deployment:

### Immediate Action
```bash
# Stop node gracefully
Ctrl+C  # Send SIGINT

# Or force kill if needed
pkill -9 cargo
```

### Revert Configuration
```bash
# Restore original config
cp neo-testnet-node.toml.backup.original neo-testnet-node.toml
```

### Revert Code Changes
```bash
# Switch back to main branch
git checkout main

# Rebuild without optimizations
cargo clean
cargo build --release  # No feature flags
```

### Restart with Original Settings
```bash
cargo run --release
```

Verify system returns to pre-optimization behavior before re-attempting deployment.

---

## Success Criteria ✨

Deployment considered successful when **ALL** criteria met:

1. ✅ Cache hit rate consistently >70% over 1-hour period
2. ✅ Blocks/sec sustained ≥200 for testnet environment
3. ✅ No crashes, panics, or consensus failures
4. ✅ Memory usage remains stable (<8GB RSS)
5. ✅ User-facing latency improved ≥15%
6. ✅ Integration tests pass with ≥95% success rate
7. ✅ No operational complaints from monitoring team

---

## Next Steps After Successful Deployment

1. **Monitor for 7 days** before promoting to production
2. **Collect real-world metrics** and compare against benchmarks
3. **Fine-tune parameters** based on actual workload patterns
4. **Document lessons learned** and add to team wiki
5. **Consider implementing Async State Root** computation (next phase)

---

## Resources & Contacts

### Documentation Links
- [Main Optimization Report](./COMPREHENSIVE-OPTIMIZATION-SUMMARY.md)
- [Tier 1 Details](./TIER1-OPTIMIZATION-SUMMARY.md)
- [Prefetch Implementation](./ACCOUNT_PREFETCH_CACHE_IMPLEMENTATION_SUMMARY.md)
- [VM Engine Optimizations](./IMPLEMENTATION-SUMMARY.md)

### Team Contacts
- Lead Engineer: Qoder
- Performance Specialist: Hank, Ivy, Jack
- Architecture Lead: Alice (Block-STM), Brian (Solana)

---

**Document Version**: 1.0  
**Last Updated**: September 14, 2026  
**Status**: Ready for Production Deployment 🚀
