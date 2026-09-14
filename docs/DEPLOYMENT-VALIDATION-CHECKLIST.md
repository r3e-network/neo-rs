# Neo-RS Performance Optimization: Deployment Validation Checklist

**Date**: September 14, 2026  
**Purpose**: Step-by-step checklist for validating optimization deployment success  

---

## Pre-Deployment Verification (Before Running Node)

### Documentation Review ✅
- [ ] Read [`COMPREHENSIVE-OPTIMIZATION-SUMMARY.md`](./COMPREHENSIVE-OPTIMIZATION-SUMMARY.md)
- [ ] Review [`QUICK_DEPLOYMENT_GUIDE.md`](./QUICK_DEPLOYMENT_GUIDE.md)
- [ ] Scan [`EXECUTIVE-SUMMARY-AND-ACTIONS.md`](./EXECUTIVE-SUMMARY-AND-ACTIONS.md)
- [ ] Bookmark [`OPTIMIZATION-QUICK-REFERENCE.md`](./OPTIMIZATION-QUICK-REFERENCE.md)

### Environment Preparation ✅
- [ ] Backup existing configuration: `cp neo-testnet-node.toml neo-testnet-node.toml.backup.original`
- [ ] Create backup directory: `mkdir -p .backup`
- [ ] Export environment variables:
  ```bash
  export ENABLE_ACCOUNT_PREFETCH=1
  export MAX_PREFETCH_CACHE_SIZE=1000
  export RUST_LOG="info,prefetch=debug,state_service=trace"
  ```
- [ ] Verify dependencies available: `command -v cargo && command -v curl`

### Code Verification ✅
- [ ] Git branch created: `git checkout -b perf-optimization-tier3`
- [ ] All optimizations committed to branch
- [ ] Unit tests passing: `cargo test --features "prefetch,runtime"`
- [ ] Build succeeds: `cargo build --release --features "prefetch,runtime"`

---

## Deployment Execution (First 1 Hour)

### Node Startup ✅
- [ ] Run node with feature flag: `cargo run --release --features "prefetch,runtime"`
- [ ] Observe initialization logs:
  - [x] ✓ "GlobalNodeCache initialized with capacity: 1M entries"
  - [x] ✓ "AccountPrefetchCache enabled with parallel loading"
  - [x] ✓ "Starting verification worker pool: X threads"
  - [x] ✓ "Metrics server started on port: 8080"

### Metrics Endpoint Availability ✅
- [ ] Wait for metrics endpoint (~30 seconds max)
- [ ] Test endpoint access: `curl http://localhost:8080/metrics`
- [ ] Verify response code is 200 (OK)
- [ ] Check response contains expected metrics

### Critical Metrics Validation ✅

#### Cache Performance (Check every 5 minutes for first hour)
- [ ] Prefetch cache hit rate ≥70%:
  ```bash
  curl http://localhost:8080/metrics | grep prefetch_hit_rate
  ```
  Expected output: `prefetch_hit_rate{cache="account"} 0.72` or higher
  
- [ ] LRU cache hit rate ≥75%:
  ```bash
  curl http://localhost:8080/metrics | grep cache_hit_rate
  ```
  Expected output: `cache_hit_rate{cache="mpt_node"} 0.78` or higher

#### Throughput Monitoring ✅
- [ ] Blocks/sec sustained ≥200 (testnet target):
  ```bash
  curl http://localhost:8080/metrics | grep blocks_per_second
  ```
  Expected: Value trending toward 200+ within first 30 minutes

- [ ] Transaction processing rate stable:
  ```bash
  curl http://localhost:8080/metrics | grep transactions_processed_total
  ```
  Expected: Counter incrementing steadily (no long pauses)

#### Resource Utilization ✅
- [ ] Memory usage <8GB RSS:
  ```bash
  free -h  # Linux
  Get-Task | Where-Object {$_.WorkingSet -gt 8GB}  # PowerShell
  ```
  Expected: Stable memory footprint, no growth over time

- [ ] CPU utilization ≥80% during peak load:
  ```bash
  top -bn1 | grep "Cpu(s)"  # Linux
  Task Manager  # Windows
  ```
  Expected: High CPU usage indicates optimization working correctly

- [ ] RocksDB I/O wait <20%:
  ```bash
  iostat -x 1  # Linux
  ```
  Expected: Low I/O wait confirms cache effectiveness

---

## First Day Validation (Hours 2-24)

### Continuous Monitoring ✅
- [ ] Check cache hit rates hourly (should stabilize >70%)
- [ ] Monitor blocks/sec trend (expect gradual increase as system warms up)
- [ ] Track transaction latency P50 (target <200ms initially)
- [ ] Watch for any panic or crash messages in logs

### Error Log Review ✅
- [ ] Zero fatal errors in application logs
- [ ] No unhandled exceptions logged
- [ ] No consensus failures recorded
- [ ] Peer connection count stable (±10% variation acceptable)

### Performance Baseline Capture ✅
- [ ] Record baseline metrics at end of Day 1:
  - Cache hit rate: _______
  - Blocks/sec average: _______
  - TX latency P50: _______
  - Memory peak usage: _______
  
### Comparison with Pre-Optimization Baseline ✅
Perform initial comparison:
- [ ] Current vs pre-opt blocks/sec ratio: _______x (expected +10x minimum)
- [ ] Current vs pre-opt latency ratio: _______x faster (expected -50%+)
- [ ] Cache effectiveness improvement: _______% points (expected +50%+)

---

## Seven-Day Validation Period

### Daily Checks Required ✅
Run these checks daily for 7 consecutive days:

#### Morning Routine ✅
- [ ] Execute validation script: `./scripts/validate_optimizations.sh`
- [ ] Review Prometheus/graphite dashboards
- [ ] Check alert thresholds not exceeded

#### Evening Summary ✅
- [ ] Document daily peak throughput achieved
- [ ] Record any performance anomalies
- [ ] Note any manual interventions required

### Weekly KPIs to Achieve ✅

By end of Week 1, ALL criteria must be met:
- [ ] **Cache Hit Rate**: Consistently >70% averaged over 7 days
- [ ] **Blocks/sec Sustained**: ≥200 average, peaks ≥250
- [ ] **TX Latency P99**: <500ms (99th percentile)
- [ ] **Memory Stability**: No OOM events, peak <8GB RSS
- [ ] **Uptime**: ≥99% availability during period
- [ ] **Error Rate**: <0.1% failed transactions
- [ ] **Consensus Participation**: 100% block proposal success

---

## Troubleshooting Validation Steps

### If Cache Hit Rate Too Low (<70%)
Perform these diagnostic steps:
- [ ] Check if prefetch feature actually enabled (re-examine logs)
- [ ] Increase `MAX_PREFETCH_CACHE_SIZE` parameter
- [ ] Reduce `PREFETCH_PARALLELISM` to reduce contention
- [ ] Validate network traffic pattern matches warm-cache assumptions

### If Memory Usage Spikes
Execute mitigation:
- [ ] Temporarily disable prefetch: `export ENABLE_ACCOUNT_PREFETCH=0`
- [ ] Reduce cache capacity to 500 entries
- [ ] Check for memory leaks via profiling tools
- [ ] Consider upgrading to NVMe SSD storage

### If Throughput Below Expectations
Diagnose bottleneck:
- [ ] Check CPU utilization (low = parallelism insufficient)
- [ ] Check RocksDB I/O (high = disk bottleneck)
- [ ] Review network bandwidth (insufficient peer connections?)
- [ ] Analyze transaction mix (too many complex contracts?)

---

## Rollback Decision Criteria

If ANY of the following occurs, initiate rollback immediately:
- [ ] ❌ System crashes persist despite troubleshooting
- [ ] ❌ Security vulnerabilities discovered in optimized code
- [ ] ❌ Protocol compatibility issues detected (block validation failures)
- [ ] ❌ Data corruption observed in state database
- [ ] ❌ Unauthorized access potential introduced by changes

### Rollback Execution Checklist
When rollback triggered:
- [ ] Stop node gracefully: `Ctrl+C` (or `pkill -9 cargo`)
- [ ] Restore configuration: `cp neo-testnet-node.toml.backup.* neo-testnet-node.toml`
- [ ] Revert git changes: `git checkout main`
- [ ] Rebuild without features: `cargo clean && cargo build --release`
- [ ] Restart with original settings
- [ ] Document root cause of rollback issue
- [ ] Schedule post-mortem meeting within 48 hours

---

## Success Declaration

After 7-day monitoring period, declare deployment successful if:

✅ **All Critical Metrics Met:**
- [ ] Average cache hit rate ≥70% throughout period
- [ ] Sustainable blocks/sec ≥200
- [ ] Zero critical incidents or data loss
- [ ] Memory and CPU within defined bounds
- [ ] Transaction latency improved ≥15% vs baseline
- [ ] User-facing services unaffected during optimization rollout

✅ **Operational Validation:**
- [ ] Operations team trained on new metrics and behaviors
- [ ] Alerting thresholds configured appropriately
- [ ] Rollback procedures rehearsed successfully
- [ ] Monitoring dashboards displaying real-time optimization metrics
- [ ] Incident response playbook updated with optimization-specific scenarios

✅ **Documentation Updated:**
- [ ] Lessons learned documented in project wiki
- [ ] Standard operating procedures revised based on actual experience
- [ ] Performance baseline established for future comparisons
- [ ] Configuration parameters optimized for production workload patterns

---

## Final Approval Sign-Off

Deployment officially approved when all stakeholders sign below:

| Role | Name | Date | Signature | Notes |
|------|------|------|-----------|-------|
| Lead Engineer | Qoder | | | |
| Architecture Reviewer | Alice/Brian | | | |
| Operations Manager | | | | |
| QA/Test Lead | | | | |
| Product Owner | | | | |

---

## Post-Validation Next Steps

Once deployment validated:
1. Plan Phase 2 implementation (Hybrid Execution Mode)
2. Evaluate Async State Root computation feasibility
3. Research Layer-2 scaling integration points
4. Community collaboration initiative launch
5. Performance tuning workshop scheduling

---

**Document Status**: Ready for Production Use  
**Review Frequency**: Update after each major deployment cycle  
**Owner**: Lead Optimization Engineer (Qoder)  
