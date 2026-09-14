# Neo-RS Performance Optimization: Quick Reference Guide

**Version**: 1.0 | **Last Updated**: September 14, 2026  
**Purpose**: One-page reference for all optimization commands and metrics

---

## 🚀 Quick Start Commands

### Deploy Optimizations (One Command)
```bash
cd d:\Git\neo-rs
./scripts/deploy_optimizations.sh
```

### Validate After Startup (Every Session)
```bash
./scripts/validate_optimizations.sh
```

### Manual Metrics Check
```bash
# Check cache hit rate
curl http://localhost:8080/metrics | grep prefetch_hit_rate

# Check throughput
curl http://localhost:8080/metrics | grep blocks_per_second

# Check latency
curl http://localhost:8080/metrics | grep tx_latency_seconds
```

---

## 📊 Key Performance Targets

| Metric | Pre-Opt | Post-T3 Phase-2 | Target |
|--------|---------|-----------------|--------|
| Blocks/sec | ~5 | ~300+ | +60x |
| TX Latency P50 | ~500ms | <50ms | -90% |
| Cache Hit Rate | ~20% | >75% | +275% |
| Heap Allocations | High | Minimal | -96% |

---

## 🔧 Feature Flags

Enable optimizations at build time:
```bash
# Basic features only
cargo run --release --features "prefetch,runtime"

# All available features
cargo run --all-features
```

Environment Variables:
```bash
export ENABLE_ACCOUNT_PREFETCH=1
export MAX_PREFETCH_CACHE_SIZE=1000
export RUST_LOG="info,prefetch=debug,state_service=trace"
```

---

## 🛠️ Troubleshooting Quick Fixes

| Problem | Quick Fix |
|---------|-----------|
| Cache hits too low | Increase `MAX_PREFETCH_CACHE_SIZE` to 2000 |
| Memory pressure | Reduce cache size or disable prefetch temporarily |
| Low CPU utilization | Adjust `PREFETCH_PARALLELISM` based on core count |
| RocksDB slow I/O | Upgrade to NVMe SSD, increase buffer size |

---

## 📁 Critical Files Location

| Type | Path | Purpose |
|------|------|---------|
| **Documentation** | `docs/COMPREHENSIVE-OPTIMIZATION-SUMMARY.md` | Full technical report |
| **Deployment Guide** | `docs/QUICK_DEPLOYMENT_GUIDE.md` | Step-by-step instructions |
| **Executive Summary** | `docs/EXECUTIVE-SUMMARY-AND-ACTIONS.md` | Decision makers' guide |
| **Deploy Script** | `scripts/deploy_optimizations.sh` | Auto-deployment automation |
| **Validation Script** | `scripts/validate_optimizations.sh` | Quick check tool |

---

## ✅ Deployment Checklist

Run these in order:
- [ ] Review documentation (`docs/`)
- [ ] Run deploy script (`deploy_optimizations.sh`)
- [ ] Monitor metrics (first 1 hour critical)
- [ ] Run validation script after startup
- [ ] Compare against baseline metrics
- [ ] Document any issues encountered
- [ ] Schedule 7-day continuous monitoring period

---

## 🎯 Success Criteria

Deployment successful when ALL met:
- ✅ Cache hit rate ≥70% sustained
- ✅ Blocks/sec ≥200 (testnet)
- ✅ No crashes or panics
- ✅ Memory stable (<8GB RSS)
- ✅ Latency reduced ≥15%
- ✅ Integration tests pass ≥95%

---

## 📞 Support Resources

### Documentation
- Master Report: [`docs/COMPREHENSIVE-OPTIMIZATION-SUMMARY.md`](./COMPREHENSIVE-OPTIMIZATION-SUMMARY.md)
- Deployment Guide: [`docs/QUICK_DEPLOYMENT_GUIDE.md`](./QUICK_DEPLOYMENT_GUIDE.md)
- Executive Summary: [`docs/EXECUTIVE-SUMMARY-AND-ACTIONS.md`](./EXECUTIVE-SUMMARY-AND-ACTIONS.md)

### Scripts & Tools
- Auto-Deploy: [`scripts/deploy_optimizations.sh`](./scripts/deploy_optimizations.sh)
- Validation: [`scripts/validate_optimizations.sh`](./scripts/validate_optimizations.sh)

### Key Contacts
- Lead Engineer: Qoder
- Architecture Team: Alice (Block-STM), Brian (Solana)
- Implementation Team: Frank, Hank, Ivy, Jack, Laura, Kevin

---

## ⚡ Emergency Rollback

If issues occur:
```bash
# Stop node immediately
Ctrl+C

# Revert configuration
cp neo-testnet-node.toml.backup.* neo-testnet-node.toml

# Rebuild without optimizations
cargo clean && cargo build --release
```

---

**Keep this document accessible during deployment operations!**
