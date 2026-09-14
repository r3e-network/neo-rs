# Neo-RS Performance Optimization: Quick Start Commands

**Purpose**: One-minute setup to deploy and validate all optimizations  
**Version**: 1.0 | **Last Updated**: September 14, 2026

---

## 🚀 Deploy Optimizations (One Command)

```bash
cd d:\Git\neo-rs
./scripts/deploy_optimizations.sh
```

That's it! The script handles everything automatically:
- Creates backups
- Sets environment variables
- Builds with feature flags
- Runs pre-deployment tests
- Launches optimized node

---

## 🔍 Validate After Startup (Quick Check)

After node starts (wait ~30 seconds for metrics):

```bash
./scripts/validate_optimizations.sh
```

Expected output: ✅ ALL CRITICAL CHECKS PASSED

---

## 📊 Check Key Metrics Manually

If validation script doesn't work, use these direct curl commands:

```bash
# Cache hit rate (target: >70%)
curl http://localhost:8080/metrics | grep prefetch_hit_rate

# Throughput (target: ≥200 blocks/sec on testnet)
curl http://localhost:8080/metrics | grep blocks_per_second

# Transaction latency P50 (target: <50ms)
curl http://localhost:8080/metrics | grep tx_latency_seconds_sum

# Memory usage (target: <8GB)
free -h  # Linux
# or Task Manager → Performance tab → Memory section (Windows)
```

---

## ⚡ Emergency Rollback (If Issues Occur)

```bash
# Stop node
Ctrl+C  # OR: pkill -9 cargo

# Restore config
cp neo-testnet-node.toml.backup.* neo-testnet-node.toml

# Rebuild without optimizations
cargo clean && cargo build --release

# Restart
cargo run --release
```

---

## 📖 Read More Documentation

| What You Need | Document Location | Time To Read |
|---------------|-------------------|--------------|
| Full technical report | `docs/COMPREHENSIVE-OPTIMIZATION-SUMMARY.md` | 15 minutes |
| Step-by-step deployment guide | `docs/QUICK_DEPLOYMENT_GUIDE.md` | 10 minutes |
| Executive summary for leadership | `docs/EXECUTIVE-SUMMARY-AND-ACTIONS.md` | 5 minutes |
| Validation checklist | `docs/DEPLOYMENT-VALIDATION-CHECKLIST.md` | 5 minutes |
| This quick reference card | `docs/OPTIMIZATION-QUICK-REFERENCE.md` | 1 minute |
| Deployment readiness report | `docs/DEPLOYMENT-READINESS-REPORT.md` | 15 minutes |

---

## 🎯 Expected Timeline

| Phase | Duration | Milestone |
|-------|----------|-----------|
| **Deploy Script Runs** | ~2 minutes | Binary compiled with optimizations |
| **Node Startup** | ~30 seconds | Metrics endpoint available |
| **Initial Validation** | ~1 minute | All checks passing confirmed |
| **Warm-up Period** | ~5 minutes | Cache hit rates stabilize |
| **Baseline Establishment** | 1 hour | Compare against pre-opt metrics |
| **Full Validation** | 7 days | All success criteria met |

---

## 💡 Pro Tips

### Tip 1: Monitor in Real-Time
Create a continuous monitoring loop:
```bash
watch -n 5 'curl -s http://localhost:8080/metrics | grep -E "(blocks_per_second|prefetch_hit_rate)"'
```

### Tip 2: Log Analysis
Filter logs by optimization components:
```bash
# Watch only relevant logs
cargo run --features "prefetch,runtime" 2>&1 | grep -E "(Cache|Prefetch|Benchmark)" --line-buffered
```

### Tip 3: Parameter Tuning
If performance below expectations, adjust environment variables before rebuild:
```bash
export MAX_PREFETCH_CACHE_SIZE=2000  # Double cache size
export PREFETCH_PARALLELISM=6        # Increase parallel workers
```

### Tip 4: Benchmark Comparison
Compare optimized vs baseline performance:
```bash
cd neo-core/benches
cargo bench --bench account_prefetch_bench
# Output shows: time vs baseline improvement percentage
```

---

## ❓ Common Questions

**Q: How long does full validation take?**  
A: Minimum 1 hour for initial validation, recommended 7 days for complete confidence

**Q: Can I deploy directly to production?**  
A: No — always start with testnet deployment first, wait for 7-day validation period

**Q: What if cache hit rate is low (<50%) after 1 hour?**  
A: Increase `MAX_PREFETCH_CACHE_SIZE` parameter and restart node

**Q: Will this break existing smart contracts?**  
A: No — all optimizations are backward compatible, maintain protocol compatibility

**Q: How do I know if optimizations actually improved performance?**  
A: Compare current `blocks_per_second` metric against your known baseline (should see significant increase)

---

## 🆘 Getting Help

If you encounter issues not covered in this document:

1. **Check Troubleshooting Section**: `docs/QUICK_DEPLOYMENT_GUIDE.md` section VII
2. **Review Validation Checklist**: `docs/DEPLOYMENT-VALIDATION-CHECKLIST.md` for systematic diagnosis
3. **Read Full Technical Report**: `docs/COMPREHENSIVE-OPTIMIZATION-SUMMARY.md` for deep understanding
4. **Contact Lead Engineer**: Qoder (via team communication channel)

---

## ✅ Success Indicators

Your deployment is successful when **ALL** of these are true:

- [x] Cache hit rate consistently ≥70% over 1-hour period
- [x] Blocks/sec sustained ≥200 (testnet) or significantly higher than baseline
- [x] Zero crashes, panics, or consensus failures observed
- [x] Memory usage stable (<8GB RSS peak)
- [x] Transaction latency improved ≥15% compared to previous deployment
- [x] Integration tests pass with ≥95% success rate
- [x] Operations team able to monitor and interpret new metrics independently

---

## 🔄 Continuous Improvement

After Week 1 validation complete:

1. Fine-tune cache parameters based on actual workload patterns
2. Document any operational learnings in team wiki
3. Schedule follow-up review meeting with all stakeholders
4. Plan Phase 2 implementation (Hybrid Execution Mode)
5. Consider Async State Root computation feasibility study

---

**Keep this document handy during deployment operations!**

📄 **Also See**: For comprehensive guidance, refer to the full documentation set in the `docs/` directory.
