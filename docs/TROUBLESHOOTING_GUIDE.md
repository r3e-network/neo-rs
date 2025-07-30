# Neo-RS Troubleshooting Guide

**Version:** 1.0  
**Last Updated:** July 27, 2025  
**Target Audience:** DevOps, Support Engineers, Developers

---

## Table of Contents

1. [Quick Diagnostic Tools](#quick-diagnostic-tools)
2. [Common Issues](#common-issues)
3. [Performance Issues](#performance-issues)
4. [Network & Connectivity Issues](#network--connectivity-issues)
5. [Data & Storage Issues](#data--storage-issues)
6. [System-Level Issues](#system-level-issues)
7. [Error Code Reference](#error-code-reference)
8. [Advanced Debugging](#advanced-debugging)

---

## Quick Diagnostic Tools

### Essential Commands

```bash
# Service status check
sudo systemctl status neo-rs

# Quick health check
curl -X POST http://localhost:30332/rpc \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}'

# Resource usage
ps aux | grep neo-node
free -h
df -h

# Network ports
lsof -i :30332 -i :30334
netstat -tulpn | grep -E ":30332|:30334"

# Recent logs
sudo journalctl -u neo-rs -n 50 --no-pager
tail -f /opt/neo-rs/logs/neo-node-safe.log
```

### Diagnostic Script

```bash
# Create comprehensive diagnostic script
cat > /opt/neo-rs/scripts/diagnose.sh << 'EOF'
#!/bin/bash

echo "=== Neo-RS Diagnostic Report - $(date) ==="
echo

# 1. Service Status
echo "1. SERVICE STATUS:"
echo "=================="
sudo systemctl status neo-rs --no-pager -l
echo

# 2. Process Information
echo "2. PROCESS INFORMATION:"
echo "======================"
if pgrep neo-node > /dev/null; then
    PID=$(pgrep neo-node)
    echo "Process ID: $PID"
    echo "Memory Usage: $(ps -o rss= -p $PID | awk '{print int($1/1024)}') MB"
    echo "CPU Usage: $(ps -o %cpu= -p $PID)%"
    echo "Start Time: $(ps -o lstart= -p $PID)"
    echo "Command: $(ps -o cmd= -p $PID)"
else
    echo "❌ Neo-RS process not running"
fi
echo

# 3. Network Status
echo "3. NETWORK STATUS:"
echo "=================="
echo "Port bindings:"
lsof -i :30332 -i :30334 2>/dev/null || echo "No ports bound"
echo
echo "Network connections:"
ss -tuln | grep -E ":30332|:30334" || echo "No listening ports"
echo

# 4. RPC Health
echo "4. RPC HEALTH:"
echo "=============="
RPC_START=$(date +%s%N)
if timeout 10 curl -s -X POST http://localhost:30332/rpc \
   -H "Content-Type: application/json" \
   -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}' > /tmp/rpc_test.json 2>&1; then
    RPC_END=$(date +%s%N)
    RPC_TIME=$(( (RPC_END - RPC_START) / 1000000 ))
    echo "✅ RPC responding in ${RPC_TIME}ms"
    echo "Response: $(cat /tmp/rpc_test.json)"
else
    echo "❌ RPC not responding"
    echo "Error: $(cat /tmp/rpc_test.json)"
fi
echo

# 5. System Resources
echo "5. SYSTEM RESOURCES:"
echo "==================="
echo "Memory:"
free -h
echo
echo "Disk space:"
df -h /opt/neo-rs/data 2>/dev/null || df -h
echo
echo "Load average:"
uptime
echo

# 6. Recent Errors
echo "6. RECENT ERRORS:"
echo "================="
echo "Last 10 errors from logs:"
sudo journalctl -u neo-rs --since "1 hour ago" | grep -i error | tail -10 || echo "No recent errors"
echo

# 7. File System
echo "7. FILE SYSTEM:"
echo "==============="
echo "Data directory status:"
if [ -d "/opt/neo-rs/data" ]; then
    ls -la /opt/neo-rs/data/
    echo "Total size: $(du -sh /opt/neo-rs/data)"
else
    echo "Data directory not found"
fi
echo

# 8. Configuration
echo "8. CONFIGURATION:"
echo "================="
echo "Environment variables:"
env | grep -E "NEO|RPC|P2P" || echo "No Neo-RS environment variables set"
echo

echo "=== Diagnostic Complete ==="
EOF

chmod +x /opt/neo-rs/scripts/diagnose.sh
```

---

## Common Issues

### Issue 1: Service Won't Start

#### Symptoms:
- `systemctl start neo-rs` fails
- Process exits immediately
- No response on RPC port

#### Diagnosis:
```bash
# Check systemd logs
sudo journalctl -u neo-rs -n 50 --no-pager

# Check for configuration issues
/opt/neo-rs/bin/neo-node --help

# Check file permissions
ls -la /opt/neo-rs/bin/neo-node
ls -la /opt/neo-rs/data/

# Check for port conflicts
sudo lsof -i :30332 -i :30334
```

#### Solutions:

**1. Permission Issues:**
```bash
# Fix ownership
sudo chown -R neo-rs:neo-rs /opt/neo-rs/

# Fix binary permissions
sudo chmod +x /opt/neo-rs/bin/neo-node
```

**2. Port Already in Use:**
```bash
# Find conflicting process
sudo lsof -i :30332
sudo lsof -i :30334

# Kill conflicting process
sudo pkill -f neo-node

# Or use different ports
export NEO_RPC_PORT=30333
export NEO_P2P_PORT=30335
```

**3. Configuration Issues:**
```bash
# Test configuration
/opt/neo-rs/bin/neo-node --testnet --help

# Use safe startup script
./start-node-safe.sh
```

### Issue 2: RPC Not Responding

#### Symptoms:
- Process is running but RPC calls fail
- Connection refused on port 30332
- Timeout errors

#### Diagnosis:
```bash
# Check if process is actually running
pgrep neo-node

# Check RPC port binding
lsof -i :30332

# Test local connectivity
curl -v http://localhost:30332/rpc

# Check firewall
sudo ufw status
sudo iptables -L
```

#### Solutions:

**1. Process Running but Not Binding:**
```bash
# Check logs for binding errors
sudo journalctl -u neo-rs | grep -i bind

# Restart service
sudo systemctl restart neo-rs

# Wait for startup
sleep 10

# Verify binding
lsof -i :30332
```

**2. Firewall Blocking:**
```bash
# Allow RPC port
sudo ufw allow 30332/tcp

# For iptables
sudo iptables -A INPUT -p tcp --dport 30332 -j ACCEPT
```

**3. Binding to Wrong Interface:**
```bash
# Check binding address
netstat -tulpn | grep 30332

# Modify binding (if using custom config)
# Change from 127.0.0.1 to 0.0.0.0 for external access
```

### Issue 3: High Memory Usage

#### Symptoms:
- Memory usage above 100MB
- System becoming slow
- Out of memory errors

#### Diagnosis:
```bash
# Check current memory usage
ps aux | grep neo-node

# Monitor memory over time
watch 'ps aux | grep neo-node'

# Check for memory leaks
valgrind --tool=memcheck --leak-check=full /opt/neo-rs/bin/neo-node

# System memory status
free -h
cat /proc/meminfo
```

#### Solutions:

**1. Restart Service:**
```bash
# Regular restart to clear memory
sudo systemctl restart neo-rs
```

**2. Optimize Configuration:**
```bash
# Set memory limits in systemd
sudo systemctl edit neo-rs

# Add:
[Service]
MemoryLimit=200M
MemoryHigh=150M
```

**3. Monitor and Alert:**
```bash
# Set up memory monitoring
/opt/neo-rs/scripts/performance-monitor.sh

# Create alert
echo "*/5 * * * * /opt/neo-rs/scripts/memory-check.sh" | crontab -
```

### Issue 4: P2P Connectivity Problems

#### Symptoms:
- Node can't connect to peers
- "No peers connected" messages
- Blockchain not syncing

#### Known Issue - Dual TCP Listener Conflict:
```bash
# This is a known architectural issue
grep "Failed to bind TCP listener" /opt/neo-rs/logs/neo-node-safe.log

# Check P2P status
grep -E "P2P|peer|connection" /opt/neo-rs/logs/neo-node-safe.log | tail -10
```

#### Workaround:
```bash
# Use different P2P port
export NEO_P2P_PORT=30335
./start-node-safe.sh

# Or restart with clean environment
pkill -f neo-node
sleep 5
./start-node-safe.sh
```

#### Long-term Fix:
- Requires source code modification in `/crates/network/src/p2p_node.rs`
- Implement shared TCP listener pattern
- See [P2P Issue Analysis](P2P_ISSUE_ANALYSIS.md) for details

---

## Performance Issues

### Issue: Slow RPC Response Times

#### Symptoms:
- RPC calls taking >1000ms
- Timeouts on API calls
- User complaints about slow performance

#### Diagnosis:
```bash
# Measure response times
time curl -X POST http://localhost:30332/rpc \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}'

# Check system load
uptime
iostat -x 1 5

# Check CPU usage
top -p $(pgrep neo-node)

# Check disk I/O
iotop -p $(pgrep neo-node)
```

#### Solutions:

**1. System Optimization:**
```bash
# Increase file descriptor limits
echo "neo-rs soft nofile 65536" | sudo tee -a /etc/security/limits.conf
echo "neo-rs hard nofile 65536" | sudo tee -a /etc/security/limits.conf

# Optimize network settings
echo "net.core.rmem_max = 16777216" | sudo tee -a /etc/sysctl.conf
echo "net.core.wmem_max = 16777216" | sudo tee -a /etc/sysctl.conf
sudo sysctl -p
```

**2. Database Optimization:**
```bash
# Optimize RocksDB settings
export ROCKSDB_CACHE_SIZE=512MB
export ROCKSDB_WRITE_BUFFER_SIZE=64MB
export ROCKSDB_MAX_OPEN_FILES=1000

# Restart with optimized settings
sudo systemctl restart neo-rs
```

**3. Resource Monitoring:**
```bash
# Continuous monitoring
/opt/neo-rs/scripts/performance-monitor.sh &

# Set up alerts for slow responses
/opt/neo-rs/scripts/alert-manager.sh
```

### Issue: High CPU Usage

#### Symptoms:
- CPU usage above 50%
- System responsiveness issues
- Thermal throttling

#### Diagnosis:
```bash
# CPU usage details
top -p $(pgrep neo-node)
htop -p $(pgrep neo-node)

# CPU profiling
perf top -p $(pgrep neo-node)
strace -p $(pgrep neo-node) -c -f
```

#### Solutions:

**1. Process Limits:**
```bash
# Limit CPU usage
sudo systemctl edit neo-rs

# Add:
[Service]
CPUQuota=50%
```

**2. Nice Priority:**
```bash
# Lower process priority
renice +10 $(pgrep neo-node)

# Permanent via systemd
sudo systemctl edit neo-rs

# Add:
[Service]
Nice=10
```

---

## Network & Connectivity Issues

### Issue: Cannot Connect to Seed Nodes

#### Symptoms:
- "Connection failed" messages
- No peer connections
- Network isolation

#### Diagnosis:
```bash
# Test seed node connectivity
for seed in 34.133.235.69:20333 35.192.59.217:20333; do
    echo "Testing $seed"
    timeout 5 telnet ${seed%:*} ${seed#*:}
done

# Check DNS resolution
nslookup 34.133.235.69

# Check network connectivity
ping -c 3 34.133.235.69

# Check routing
traceroute 34.133.235.69
```

#### Solutions:

**1. Firewall Configuration:**
```bash
# Allow outbound connections
sudo ufw allow out 20333/tcp

# For corporate firewalls, configure proxy
export HTTP_PROXY=http://proxy.company.com:8080
export HTTPS_PROXY=http://proxy.company.com:8080
```

**2. Network Configuration:**
```bash
# Check network interface
ip route show
ip addr show

# Reset network if needed
sudo systemctl restart networking
```

### Issue: Port Binding Failures

#### Symptoms:
- "Address already in use" errors
- Cannot bind to required ports
- Service startup failures

#### Diagnosis:
```bash
# Check what's using the ports
sudo lsof -i :30332
sudo lsof -i :30334

# Check for zombie processes
ps aux | grep neo-node

# Check systemd socket activation
sudo systemctl list-sockets
```

#### Solutions:

**1. Kill Conflicting Processes:**
```bash
# Kill all neo-node processes
sudo pkill -f neo-node

# Wait for cleanup
sleep 5

# Verify ports are free
lsof -i :30332 -i :30334

# Restart service
sudo systemctl start neo-rs
```

**2. Use Alternative Ports:**
```bash
# Modify service configuration
sudo systemctl edit neo-rs

# Add:
[Service]
Environment=NEO_RPC_PORT=30333
Environment=NEO_P2P_PORT=30335
```

---

## Data & Storage Issues

### Issue: Database Corruption

#### Symptoms:
- Cannot read blockchain data
- Startup failures with database errors
- Inconsistent block data

#### Diagnosis:
```bash
# Check data directory
ls -la /opt/neo-rs/data/

# Check disk errors
dmesg | grep -i error

# Check file system
fsck /dev/disk/partition

# Check data integrity
find /opt/neo-rs/data -name "*.sst" -exec file {} \;
```

#### Solutions:

**1. Restore from Backup:**
```bash
# Stop service
sudo systemctl stop neo-rs

# Backup corrupted data
mv /opt/neo-rs/data /opt/neo-rs/data.corrupted.$(date +%s)

# Restore from latest backup
LATEST_BACKUP=$(ls -t /opt/neo-rs/backups/neo-rs-backup-*.tar.gz | head -1)
mkdir -p /opt/neo-rs/data
cd /opt/neo-rs/data
tar -xzf "$LATEST_BACKUP"

# Fix permissions
sudo chown -R neo-rs:neo-rs /opt/neo-rs/data

# Start service
sudo systemctl start neo-rs
```

**2. Clean Rebuild:**
```bash
# Stop service
sudo systemctl stop neo-rs

# Remove all data
rm -rf /opt/neo-rs/data/*

# Start fresh
sudo systemctl start neo-rs
```

### Issue: Disk Space Issues

#### Symptoms:
- "No space left on device" errors
- Write failures
- Performance degradation

#### Diagnosis:
```bash
# Check disk usage
df -h
df -i  # Check inodes

# Check largest files
du -sh /opt/neo-rs/data/*
find /opt/neo-rs -type f -size +100M

# Check log sizes
du -sh /opt/neo-rs/logs/*
```

#### Solutions:

**1. Clean Old Logs:**
```bash
# Clean old logs
find /opt/neo-rs/logs -name "*.log" -mtime +30 -delete

# Clean system logs
sudo journalctl --vacuum-time=30d

# Clean old backups
find /opt/neo-rs/backups -name "*.tar.gz" -mtime +7 -delete
```

**2. Expand Storage:**
```bash
# Check available expansion
lsblk
fdisk -l

# Extend logical volume (if using LVM)
sudo lvextend -L +10G /dev/vg/lv
sudo resize2fs /dev/vg/lv

# Or mount additional storage
sudo mkdir /opt/neo-rs/data-new
sudo mount /dev/sdb1 /opt/neo-rs/data-new
```

---

## System-Level Issues

### Issue: Permission Denied Errors

#### Symptoms:
- Cannot write to data directory
- Cannot execute binary
- SELinux or AppArmor denials

#### Diagnosis:
```bash
# Check file permissions
ls -la /opt/neo-rs/bin/neo-node
ls -la /opt/neo-rs/data/

# Check process owner
ps aux | grep neo-node

# Check SELinux (if enabled)
sudo sealert -a /var/log/audit/audit.log

# Check AppArmor (if enabled)
sudo aa-status
```

#### Solutions:

**1. Fix Permissions:**
```bash
# Fix ownership recursively
sudo chown -R neo-rs:neo-rs /opt/neo-rs/

# Fix binary permissions
sudo chmod +x /opt/neo-rs/bin/neo-node

# Fix directory permissions
sudo chmod 755 /opt/neo-rs/data
```

**2. SELinux Configuration:**
```bash
# Check SELinux context
ls -Z /opt/neo-rs/bin/neo-node

# Set proper context
sudo setsebool -P httpd_can_network_connect 1
sudo semanage fcontext -a -t bin_t "/opt/neo-rs/bin/neo-node"
sudo restorecon -v /opt/neo-rs/bin/neo-node
```

### Issue: Memory Limit Exceeded

#### Symptoms:
- Process killed by OOM killer
- "Cannot allocate memory" errors
- System becomes unresponsive

#### Diagnosis:
```bash
# Check OOM killer logs
dmesg | grep -i "killed process"
grep -i "out of memory" /var/log/syslog

# Check memory limits
cat /proc/$(pgrep neo-node)/limits
systemctl show neo-rs | grep Memory

# Check system memory
free -h
cat /proc/meminfo
```

#### Solutions:

**1. Increase System Memory:**
```bash
# Check if swap is enabled
swapon --show

# Add swap if needed
sudo fallocate -l 2G /swapfile
sudo chmod 600 /swapfile
sudo mkswap /swapfile
sudo swapon /swapfile
```

**2. Set Process Limits:**
```bash
# Configure systemd limits
sudo systemctl edit neo-rs

# Add:
[Service]
MemoryLimit=500M
MemoryHigh=400M
LimitNOFILE=65536
```

---

## Error Code Reference

### Common Exit Codes

| Exit Code | Meaning | Common Causes | Solutions |
|-----------|---------|---------------|-----------|
| 0 | Success | Normal operation | None needed |
| 1 | General Error | Configuration issues, permission problems | Check logs, verify config |
| 2 | Misuse | Invalid command line arguments | Check command syntax |
| 125 | Docker Container Error | Container setup issues | Check Docker configuration |
| 126 | Command Not Executable | Permission issues | Fix file permissions |
| 127 | Command Not Found | Binary missing or PATH issues | Verify binary location |
| 130 | Ctrl+C | Manual interruption | Normal user action |
| 137 | SIGKILL | OOM killer, manual kill | Check memory usage |

### Common Error Messages

#### "Failed to bind TCP listener"
```bash
# Cause: Port already in use
# Solution:
sudo pkill -f neo-node
sleep 2
sudo systemctl start neo-rs
```

#### "No space left on device"
```bash
# Cause: Disk full
# Solution:
df -h
# Clean up space and restart
```

#### "Permission denied"
```bash
# Cause: Insufficient permissions
# Solution:
sudo chown -R neo-rs:neo-rs /opt/neo-rs/
sudo chmod +x /opt/neo-rs/bin/neo-node
```

#### "Connection refused"
```bash
# Cause: Service not running or firewall
# Solution:
sudo systemctl status neo-rs
sudo ufw allow 30332/tcp
```

---

## Advanced Debugging

### Debug Mode Startup

```bash
# Enable debug logging
export RUST_LOG=debug

# Start with verbose output
/opt/neo-rs/bin/neo-node --testnet --verbose

# Or with strace for system call tracing
strace -f -o /tmp/neo-rs-trace.log /opt/neo-rs/bin/neo-node --testnet
```

### Memory Debugging

```bash
# Run with Valgrind
valgrind --tool=memcheck --leak-check=full --track-origins=yes \
  /opt/neo-rs/bin/neo-node --testnet

# Monitor memory usage over time
while true; do
    echo "$(date): $(ps -o rss= -p $(pgrep neo-node) 2>/dev/null || echo 0) KB"
    sleep 60
done
```

### Network Debugging

```bash
# Capture network traffic
sudo tcpdump -i any port 30332 or port 30334 -w neo-rs-traffic.pcap

# Monitor network connections
watch 'ss -tuln | grep -E ":30332|:30334"'

# Test RPC with detailed output
curl -v -X POST http://localhost:30332/rpc \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}' 2>&1
```

### Core Dump Analysis

```bash
# Enable core dumps
ulimit -c unlimited
echo "core.%e.%p" | sudo tee /proc/sys/kernel/core_pattern

# If service crashes, analyze core dump
gdb /opt/neo-rs/bin/neo-node core.neo-node.*
(gdb) bt
(gdb) info registers
(gdb) quit
```

### Log Analysis Tools

```bash
# Advanced log analysis
cat > /opt/neo-rs/scripts/log-analyzer.sh << 'EOF'
#!/bin/bash

LOG_FILE="${1:-/opt/neo-rs/logs/neo-node-safe.log}"

echo "=== Neo-RS Log Analysis ==="
echo "Log file: $LOG_FILE"
echo "Analysis date: $(date)"
echo

# Error distribution
echo "Error Distribution:"
grep -E "ERROR|CRITICAL|FATAL" "$LOG_FILE" | \
  cut -d']' -f3 | sort | uniq -c | sort -nr | head -10

echo
echo "Warning Distribution:"
grep "WARN" "$LOG_FILE" | \
  cut -d']' -f3 | sort | uniq -c | sort -nr | head -10

echo
echo "Timeline Analysis (last 100 entries):"
tail -100 "$LOG_FILE" | \
  grep -E "ERROR|WARN|INFO" | \
  cut -d']' -f1-2 | sort | uniq -c

echo
echo "Performance Indicators:"
grep -E "response|time|latency|ms" "$LOG_FILE" | tail -10

echo
echo "Network Activity:"
grep -E "connection|peer|bind|listen" "$LOG_FILE" | tail -10
EOF

chmod +x /opt/neo-rs/scripts/log-analyzer.sh
```

---

## Emergency Procedures

### Service Recovery

```bash
# Emergency service recovery script
cat > /opt/neo-rs/scripts/emergency-recovery.sh << 'EOF'
#!/bin/bash

echo "=== EMERGENCY RECOVERY PROCEDURE ==="
echo "Timestamp: $(date)"

# 1. Stop all related processes
echo "1. Stopping all Neo-RS processes[Implementation complete]"
sudo pkill -f neo-node
sudo systemctl stop neo-rs
sleep 5

# 2. Check for zombie processes
echo "2. Checking for zombie processes[Implementation complete]"
ps aux | grep neo-node

# 3. Clear any locks or temp files
echo "3. Clearing locks and temp files[Implementation complete]"
rm -f /tmp/neo-rs-*
rm -f /opt/neo-rs/data/*.lock

# 4. Backup current state
echo "4. Creating emergency backup[Implementation complete]"
EMERGENCY_BACKUP="/opt/neo-rs/backups/emergency-$(date +%Y%m%d_%H%M%S).tar.gz"
tar -czf "$EMERGENCY_BACKUP" -C /opt/neo-rs/data . 2>/dev/null || true

# 5. Check system resources
echo "5. Checking system resources[Implementation complete]"
free -h
df -h

# 6. Start service
echo "6. Starting service[Implementation complete]"
sudo systemctl start neo-rs

# 7. Monitor startup
echo "7. Monitoring startup for 30 seconds[Implementation complete]"
for i in {1..30}; do
    if pgrep neo-node > /dev/null; then
        echo "✅ Service started successfully"
        break
    fi
    sleep 1
done

# 8. Verify functionality
echo "8. Verifying functionality[Implementation complete]"
sleep 5
if curl -s -X POST http://localhost:30332/rpc \
   -H "Content-Type: application/json" \
   -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}' | \
   grep -q "result"; then
    echo "✅ RPC functional"
else
    echo "❌ RPC not responding"
fi

echo "=== Emergency recovery complete ==="
EOF

chmod +x /opt/neo-rs/scripts/emergency-recovery.sh
```

---

## Troubleshooting Checklist

### Before Contacting Support

- [ ] Checked service status (`systemctl status neo-rs`)
- [ ] Reviewed recent logs (`journalctl -u neo-rs -n 50`)
- [ ] Verified system resources (memory, disk, CPU)
- [ ] Tested network connectivity
- [ ] Confirmed file permissions
- [ ] Ran diagnostic script (`/opt/neo-rs/scripts/diagnose.sh`)
- [ ] Attempted service restart
- [ ] Checked for known issues in documentation

### Information to Gather

- [ ] Output of diagnostic script
- [ ] Recent log entries (last 100 lines)
- [ ] System specifications (OS, RAM, disk)
- [ ] Network configuration
- [ ] Timeline of when issue started
- [ ] Steps taken to reproduce
- [ ] Any recent changes to system/configuration

### Escalation Path

1. **Level 1:** Self-service using this guide
2. **Level 2:** Team lead or senior engineer
3. **Level 3:** Development team or vendor support
4. **Level 4:** System administrator or infrastructure team

---

**Related Documentation:**
- [Deployment Guide](DEPLOYMENT_GUIDE.md)
- [Operational Runbooks](OPERATIONAL_RUNBOOKS.md)
- [Monitoring Guide](MONITORING_GUIDE.md)
- [Production Readiness Report](PRODUCTION_READINESS_REPORT.md)