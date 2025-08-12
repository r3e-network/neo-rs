# Neo Rust Automated TestNet Sync Verification

## Overview

This document describes the automated tools for monitoring and validating Neo Rust node synchronization with the Neo N3 TestNet.

## 🚀 Quick Start

```bash
# Start sync monitoring
./scripts/testnet_sync_monitor.sh

# Run validation tests
./scripts/validate_sync.sh
```

## 📊 Sync Monitor (`testnet_sync_monitor.sh`)

The sync monitor continuously tracks your node's synchronization progress and health.

### Features

- **Real-time Monitoring**: Updates every 60 seconds
- **Sync Progress Tracking**: Shows blocks synced, rate, and ETA
- **Health Checks**: Monitors node connectivity and performance
- **Automated Alerts**: Email notifications for issues
- **Detailed Logging**: Complete history of sync progress

### Usage

```bash
# Basic usage with defaults
./scripts/testnet_sync_monitor.sh

# Custom configuration
./scripts/testnet_sync_monitor.sh \
    --rpc http://localhost:20332 \
    --reference https://seed1t5.neo.org:20332 \
    --interval 30 \
    --email admin@example.com
```

### Options

| Option | Description | Default |
|--------|-------------|---------|
| `--rpc` | Local node RPC URL | http://localhost:20332 |
| `--reference` | Reference node RPC URL | https://seed1t5.neo.org:20332 |
| `--interval` | Check interval (seconds) | 60 |
| `--email` | Email for alerts | None |

### Sample Output

```
=====================================
Neo TestNet Sync Monitor Report
=====================================
Time: Mon Aug 11 10:30:45 UTC 2025
-------------------------------------
Node Status:
  Local Height:     1234567
  Reference Height: 1234570
  Difference:       3 blocks
  Sync Status:      synced
  Sync Rate:        2.5 blocks/sec
  ETA to Sync:      N/A
  
Network Status:
  Connected Peers:  8
  
Health Status:    healthy
Issues:           None
=====================================
```

### Health States

- **healthy**: Node is fully synced and operating normally
- **warning**: Minor issues (low peers, slow sync)
- **critical**: Major issues (node not responding, sync stalled)

## 🔍 Sync Validator (`validate_sync.sh`)

The validator performs comprehensive tests to ensure your node is correctly synchronized.

### Test Suite

1. **Block Height Consistency**: Verifies height matches reference nodes
2. **Block Hash Verification**: Ensures block hashes match network
3. **Transaction Verification**: Validates transaction data integrity
4. **State Root Verification**: Checks state synchronization
5. **Network Connectivity**: Confirms peer connections
6. **Consensus Messages**: Verifies consensus participation
7. **Native Contracts**: Tests contract accessibility
8. **Memory Pool**: Checks mempool functionality
9. **Performance Metrics**: Measures RPC response times
10. **Data Integrity**: Random block verification

### Usage

```bash
# Run all validation tests
./scripts/validate_sync.sh

# Specify custom RPC endpoint
./scripts/validate_sync.sh --rpc http://localhost:20332
```

### Test Results

Each test produces one of three results:

- ✓ **PASS**: Test completed successfully
- ✗ **FAIL**: Test failed, investigation needed
- ⚠ **WARN**: Test passed with minor issues

### Sample Output

```
Neo TestNet Sync Validation
==================================
Local Node: http://localhost:20332
Start Time: Mon Aug 11 10:35:00 UTC 2025

Test 1: Block Height Consistency
✓ Block height consistent across network (Height: 1234567)

Test 2: Block Hash Verification
✓ Block hash matches at height 1234557

Test 3: Transaction Verification
✓ Transaction 0x123...abc verified

Test 4: State Root Verification
✓ State height synchronized (Height: 1234566)

Test 5: Network Connectivity
✓ Connected to 8 peers

Test 6: Consensus Message Reception
✓ Receiving consensus messages (Count: 152)

Test 7: Native Contract Accessibility
✓ NEO native contract accessible
✓ GAS native contract accessible

Test 8: Memory Pool Functionality
✓ Memory pool functional (Size: 12)

Test 9: Performance Metrics
✓ RPC response time: 45ms

Test 10: Data Integrity Check
✓ Block 987654 data integrity verified

Validation Summary
==================
Passed:   12
Failed:   0
Warnings: 0

✓ VALIDATION PASSED - Node is fully synchronized!
```

## 🤖 Automation Integration

### Systemd Service

Create `/etc/systemd/system/neo-sync-monitor.service`:

```ini
[Unit]
Description=Neo TestNet Sync Monitor
After=neo-testnet.service

[Service]
Type=simple
ExecStart=/opt/neo-rs/scripts/testnet_sync_monitor.sh
Restart=always
StandardOutput=append:/var/log/neo-sync-monitor.log
StandardError=append:/var/log/neo-sync-monitor.log

[Install]
WantedBy=multi-user.target
```

Enable and start:

```bash
sudo systemctl enable neo-sync-monitor
sudo systemctl start neo-sync-monitor
```

### Cron Job for Validation

Add to crontab:

```bash
# Run validation every hour
0 * * * * /opt/neo-rs/scripts/validate_sync.sh >> /var/log/neo-sync-validation.log 2>&1
```

### CI/CD Integration

```yaml
# .github/workflows/sync-check.yml
name: TestNet Sync Check

on:
  schedule:
    - cron: '0 */6 * * *'  # Every 6 hours

jobs:
  sync-validation:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v2
      
      - name: Start Neo Node
        run: |
          docker run -d --name neo-test \
            -p 20332:20332 \
            neo-rust:testnet
      
      - name: Wait for Initial Sync
        run: sleep 300
      
      - name: Run Sync Validation
        run: |
          ./scripts/validate_sync.sh --rpc http://localhost:20332
```

## 📈 Monitoring Dashboard Integration

### Prometheus Metrics Export

Add to sync monitor for Prometheus integration:

```bash
# Export metrics to file for node_exporter
cat > /var/lib/node_exporter/neo_sync.prom <<EOF
# HELP neo_sync_height Current sync height
# TYPE neo_sync_height gauge
neo_sync_height $local_height

# HELP neo_sync_lag Blocks behind reference
# TYPE neo_sync_lag gauge
neo_sync_lag $((reference_height - local_height))

# HELP neo_sync_rate Sync rate in blocks per second
# TYPE neo_sync_rate gauge
neo_sync_rate $sync_rate

# HELP neo_sync_peers Connected peer count
# TYPE neo_sync_peers gauge
neo_sync_peers $peers
EOF
```

### Grafana Alert Rules

```json
{
  "alert": "NeoSyncLag",
  "expr": "neo_sync_lag > 100",
  "for": "5m",
  "annotations": {
    "summary": "Neo node is {{ $value }} blocks behind"
  }
}
```

## 🔧 Troubleshooting

### Common Issues

**Sync Stalled**
```bash
# Check disk space
df -h

# Check network connectivity
./scripts/testnet_sync_monitor.sh --interval 10

# Restart with clean state
systemctl stop neo-testnet
rm -rf testnet-data/chain.db
systemctl start neo-testnet
```

**Validation Failures**
```bash
# Check specific test
./scripts/validate_sync.sh 2>&1 | grep -A5 "FAIL"

# Verify RPC is accessible
curl -X POST http://localhost:20332 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getblockcount","params":[],"id":1}'
```

**Performance Issues**
```bash
# Monitor resource usage during sync
./scripts/testnet_sync_monitor.sh &
htop
```

## 📊 Metrics and Reporting

### Generate Weekly Report

```bash
#!/bin/bash
# weekly_report.sh

echo "Neo Sync Weekly Report - $(date)"
echo "================================"

# Average sync rate
grep "Rate:" testnet_sync_monitor.log | \
  awk '{sum+=$NF; count++} END {print "Avg Sync Rate:", sum/count, "blocks/sec"}'

# Validation success rate
success=$(grep -c "VALIDATION PASSED" sync_validation.log)
total=$(grep -c "Starting Neo TestNet Sync Validation" sync_validation.log)
echo "Validation Success Rate: $((success * 100 / total))%"

# Downtime
grep "ERROR: Cannot connect" testnet_sync_monitor.log | wc -l
```

## 🚀 Best Practices

1. **Run Continuous Monitoring**: Keep sync monitor running 24/7
2. **Regular Validation**: Run validator at least hourly
3. **Set Up Alerts**: Configure email/Slack notifications
4. **Monitor Logs**: Review logs for patterns
5. **Baseline Metrics**: Establish normal operating parameters
6. **Automate Recovery**: Script automatic restart on failures

## 📚 Additional Resources

- [Neo TestNet Explorer](https://testnet.neoscan.io/)
- [Neo RPC Documentation](https://docs.neo.org/docs/en-us/reference/rpc/latest-version/api.html)
- [Monitoring Setup Guide](./MONITORING_SETUP.md)

---

For issues or improvements, please submit a GitHub issue or pull request.