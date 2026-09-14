# Neo-N3 Testnet Sync - Quick Reference Card

## 🚀 30-Second Setup

```bash
# 1. Build optimized binary
cargo build --release --bin neo-node

# 2. Run full sync
./tools/sync-monitor/sync.sh sync   # Linux/Mac
tools\sync-monitor\sync.bat sync    # Windows

# 3. Open dashboard
http://localhost:8080
```

---

## ⚡ Common Commands

| Action | Command |
|--------|---------|
| Full sync from genesis | `./tools/sync-monitor/sync.sh sync` |
| Resume from block #5M | `./tools/sync-monitor/sync.sh sync 5000000` |
| Simulation mode | `./tools/sync-monitor/sync.sh test` |
| Windows equivalent | `tools\sync-monitor\sync.bat sync` |
| Stop monitoring | Press `Ctrl+C` |

---

## 📊 Monitor Status

### Check current progress via RPC
```cmd
curl -X POST -d '{"jsonrpc":"2.0","id":1,"method":"getblockcount","params":[]}' http://localhost:10332
```

### View logs in real-time
```bash
tail -f logs/*.log | grep Block
```

### Dashboard access
```
http://localhost:8080
```

---

## 🎯 Expected Performance

**Hardware:** Modern laptop/PC with SSD, 16GB RAM  
**Total Blocks:** ~10 million (full testnet history)  
**Optimized Time:** 18-24 hours  
**Baseline Time:** 48-72 hours  
**Speedup:** 2-3x faster

---

## 🔍 Validation Points

- ✅ Every 1,000 blocks → State root check
- ✅ Every 100,000 blocks → Milestone celebration
- ✅ Continuous → Protocol consistency verification
- ✅ Auto-recovery → Divergence handling

---

## 🛠️ Troubleshooting

| Issue | Solution |
|-------|----------|
| Slow sync (<10 blk/s) | Increase MaxPeers; Use SSD; Close bandwidth apps |
| No peers connecting | Check firewall port 20332; Restart node |
| Out of memory | Reduce MemoryBytes in config to 32MB |
| State divergence | System auto-recovers; Check reports folder |

---

## 📁 Generated Files

```
logs/neo-sync-YYYYMMDD-HHMMSS.log   # Complete sync log
reports/final-sync-report-*.md      # Summary report
reports/sync-report.json            # Structured data
```

---

## ✅ Success Checklist

- [ ] Reached testnet tip height (~10M+)
- [ ] Zero unrecovered divergences
- [ ] All validation checks passed ✓
- [ ] Completion report generated
- [ ] Dashboard shows 100% progress

---

## 💡 Pro Tips

- **Fastest:** NVMe SSD + 100+ peers + modern CPU
- **Quietest:** Redirect output to file, monitor logs
- **Safest:** Daily backups of state directory
- **Easiest:** Let script handle everything automatically

---

*Need more details? See README.md or QUICK_START.md*
