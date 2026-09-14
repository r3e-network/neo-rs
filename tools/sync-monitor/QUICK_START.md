# Neo-N3 Testnet Sync - Quick Start Guide

## 🚀 Get Started in 3 Steps

### Step 1: Build Optimized Binary

```bash
# Ensure you're at the project root
cd d:\Git\neo-rs

# Build with all optimizations enabled
cargo build --release --bin neo-node

# Verify binary was created
dir target\release\neo-node.exe
```

**Expected:** Release binary ready (~5-10 minutes depending on hardware)

---

### Step 2: Choose Your Sync Method

#### Option A: Windows Batch Script (Recommended for Windows)

```cmd
REM Full sync from genesis
tools\sync-monitor\sync.bat sync

REM Resume from specific block height
tools\sync-monitor\sync.bat sync 5000000

REM Simulation mode (no actual node)
tools\sync-monitor\sync.bat test
```

#### Option B: Linux/Bash Script (WSL/Linux/Mac)

```bash
chmod +x tools/sync-monitor/sync.sh

# Full sync from genesis
./tools/sync-monitor/sync.sh sync

# Resume from block #5,000,000
./tools/sync-monitor/sync.sh sync 5000000

# Run simulation
./tools/sync-monitor/sync.sh test
```

#### Option C: Manual Control (Advanced Users)

```cmd
REM Terminal 1: Start neo-node
target\release\neo-node.exe --config config\testnet-production.toml

REM Terminal 2: Monitor logs
tail -f logs\*.log | findstr /C:"Block" /C:"state"
```

---

## 📊 Monitoring Dashboard

After starting sync, access the web dashboard:

**URL:** `http://localhost:8080`

### What You'll See:

✅ **Real-Time Progress**
- Current block number vs total testnet height
- Visual progress bar (0% → 100%)
- Live speed metrics (blocks/second)
- Estimated time remaining

✅ **State Validation Panel**
- Checkpoint validations every 1,000 blocks
- Pass/fail status for each validation
- Recent checkpoint list

✅ **Protocol Consistency Checks**
- Transaction execution verification ✓
- Syscall gas cost validation ✓
- Native contract integrity ✓
- C# reference matching ✓

✅ **Live Console Output**
- Real-time neo-node log stream
- Timestamped entries by severity
- Auto-scroll toggle
- Clear button

✅ **Milestone Tracking**
- Animated milestones every 100,000 blocks
- Completed milestones marked green
- Current position highlighted

---

## 🎯 Expected Timeline

With optimized binary and modern hardware:

| Blocks | Duration | Milestone |
|--------|----------|-----------|
| 0 → 100K | ~2 hours | Early Stage |
| 0 → 1M | ~20 hours | Mid Era |
| 0 → 5M | ~60 hours | Current Era |
| 0 → 10M | ~18-24 hours* | Full History |

*\*Timeline varies based on:*
- Hardware (SSD vs HDD, CPU, RAM)
- Network connectivity
- Peer availability
- Temperature/throttling

---

## 🔍 During Sync

### What Happens Automatically:

1. **Every Block**: 
   - Download and verify
   - Execute transactions
   - Update state tree
   - Log progress

2. **Every 1,000 Blocks** (Checkpoint):
   - Validate state root
   - Compare MPT integrity
   - Cross-reference consensus
   - Log success/failure

3. **Every 100,000 Blocks** (Milestone):
   - Generate notification
   - Update dashboards
   - Create report entry
   - Optional webhook alert

4. **Continuous Monitoring**:
   - Track peer count
   - Measure sync speed
   - Calculate ETA
   - Detect anomalies

---

## ⚠️ Troubleshooting Common Issues

### Issue: Sync is slower than expected (<10 blk/s)

**Possible Causes:**
- Using HDD instead of SSD
- Low peer count (<10 peers)
- System resource constraints

**Solutions:**
```toml
// Increase max peers in config/testnet-production.toml
[P2P]
MaxPeers = 150

// Use SSD-backed storage path
[Storage]
LevelDB = { Path = "D:\SSD\data\testnet" }
```

---

### Issue: Node won't start or no peers connect

**Check:**
```cmd
REM Check if port is open
netstat -an | findstr "20332"

REM Check firewall settings
netsh advfirewall show allprofiles

REM Try restarting with fresh peers
taskkill /F /IM neo-node.exe
(Then restart)
```

---

### Issue: State divergence detected

**Symptoms:**
```
✗ Divergence at block #1,234,567
  Computed: abc123...
  Expected: def456...
```

**Auto-Recovery Actions:**
- ✅ Halts synchronization immediately
- ✅ Rolls back to last valid checkpoint (#1,233,000)
- ✅ Generates forensic report in `reports/`
- ✅ Retries with re-validation

**Manual Intervention (if needed):**
```cmd
REM Delete corrupted state data
rmdir /s target\testnet\StateService\*

REM Resume from safe checkpoint
tools\sync-monitor\sync.bat sync 1233000
```

---

### Issue: Out of memory or disk space errors

**Quick Fixes:**
```toml
// Reduce TxPool memory allocation
[TxPool]
MemoryBytes = 33554432  // 32MB instead of 64MB

// Use SQLite for smaller footprint
[Storage]
SQLite = { Path = "data/testnet-small.db" }
```

---

## 🏁 After Completion

### Final Validation Checklist:

```cmd
REM 1. Verify final height
curl -X POST -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getblockcount\",\"params\":[]}" http://localhost:10332

REM Result should show current testnet tip height

REM 2. Check recent state roots
curl -X POST -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getstateroot\",\"params\":[YOUR_FINAL_BLOCK]}\n\"} http://localhost:10332

REM 3. Review generated reports
type reports\final-sync-report-*.md

REM 4. Inspect detailed logs
notepad logs\neo-sync-*.log
```

### Generated Files:

📁 **Location:** `logs/neo-sync-[TIMESTAMP].log`
- Complete node output
- Every synced block logged
- Error details if any

📁 **Location:** `reports/final-sync-report-[TIMESTAMP].md`
- Executive summary
- Performance statistics
- Validation results
- Compliance checklist

📁 **Location:** `reports/sync-report.json`
- Structured machine-readable data
- All metrics and timestamps
- Automation-friendly format

---

## 🎨 Dashboard Screenshots Reference

### Main View Layout:

```
┌─────────────────────────────────────────────────────────────┐
│                    Neo-N3 Testnet Sync                      │
│              Real-time Synchronization & Validation         │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  [Progress Overview]                                        │
│  ████████████░░░░░░░░░░░░░░░░░░░░░░░░░░░░  45.67%          │
│  Blocks: 4,567,890 / 10,000,000 | Speed: 52.3/s            │
│                                                             │
│  [Key Metrics]      [State Validation]     [Protocol Checks]│
│  ┌──────────┐      ┌──────────────────┐    ┌────────────┐ │
│  │ 52 blk/s │      │ ✓ #4,567,000     │    │ ✓ Transactions │
│  │✓ Passed  │      │ ✓ #4,566,000     │    │ ✓ Gas Costs    │
│  │Runtime   │      │ ✓ #4,565,000     │    │ ✓ Native Contracts│
│  │00:45:23  │      │ ✗ #4,500,000     │    │ ✓ C# Match     │
│  └──────────┘      └──────────────────┘    └────────────┘ │
│                                                             │
│  [Milestones]                                               │
│  • #0       • #100K    • #1M      ● #4.5M     • #10M       │
│  ✓          ✓          ✓           ↑             ◦          │
│                                                             │
│  [Live Console]                                             │
│  [14:23:45] [INFO] Block #4,567,890 synced                │
│  [14:23:44] [INFO] State validated at #4,567,000           │
│  [14:23:43] [WARN] Minor delay, retrying...               │
│  ...                                                      │
└─────────────────────────────────────────────────────────────┘
```

---

## 🛠️ Advanced Configuration

### Custom RPC Port (if 10332 is taken)

```toml
# config/testnet-production.toml
[RPC]
Port = 10333  # Changed from default 10332
```

Then update monitoring scripts to use new port.

### Adjust Validation Frequency

Modify in `sync_monitor.py`:

```python
class SyncMonitor:
    CHECKPOINT_INTERVAL = 500  # Validate every 500 blocks (default: 1000)
```

### Enable Verbose Logging

```toml
# config/testnet-production.toml
[Logger]
LogLevel = "Debug"  # Options: Debug, Info, Warn, Error
Path = "logs/debug"
```

---

## 💡 Pro Tips

### Tip 1: Optimize for Speed
```toml
[Plugins]
# Enable all performance plugins
EnableStateService = true
EnableP2PState = true
```

### Tip 2: Monitor Resources
```cmd
REM Windows Task Manager
PERMONITOR.EXE cpu mem disk net

REM Linux
top -p $(pgrep neo-node)
```

### Tip 3: Backup Progress
```cmd
REM Daily automated backup of state
robocopy target\testnet BACKUP_DIR /MIR
```

### Tip 4: Multiple Nodes
Want redundancy? Run multiple nodes on different ports:

```cmd
node1: cargo run --release -- --config config/testnet-port1.toml
node2: cargo run --release -- --config config/testnet-port2.toml
```

Each uses different P2P port (20332, 20333).

---

## 📞 Need Help?

### Quick Diagnostics
```cmd
REM Health check script
tools\sdk\node-health-check.ps1
```

### Community Support
- GitHub Issues: https://github.com/cityofzion/neo/issues
- Forum: https://forum.neo.org/
- Discord: https://discord.gg/neo

### Documentation
- Neo Docs: https://docs.neo.org/
- N3 Specs: https://docs.neo.org/docs/en-us/reference/n3/latest/index.html

---

## ✅ Success Criteria

Your sync was successful if you see:

1. ✅ Reached current testnet tip height
2. ✅ Zero state divergences (or auto-recovered)
3. ✅ All protocol checks passed
4. ✅ Validated checkpoints match C# implementation
5. ✅ Generated completion report

---

## 🎉 Congratulations!

You've successfully synced your optimized Neo-N3 node to full testnet history!

Your node now has:
- Complete historical state
- Verified cryptographic proofs
- Production-ready performance
- Full protocol compliance

**Ready for production deployment!** 🚀

---

*Generated: September 2026 | Neo-RS Optimization Suite v1.0*
