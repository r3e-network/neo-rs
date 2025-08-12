# Neo Rust Node Incident Response Playbook

## Overview

This playbook provides step-by-step procedures for responding to various incidents affecting Neo Rust nodes. Each incident type includes detection, triage, resolution, and post-incident review procedures.

## 🚨 Incident Classification

### Severity Levels

| Level | Description | Response Time | Examples |
|-------|-------------|---------------|----------|
| **P1 - Critical** | Complete outage or data loss risk | 15 min | Node crash, consensus failure, data corruption |
| **P2 - High** | Major degradation or partial outage | 30 min | Network partition, high error rate, sync failure |
| **P3 - Medium** | Minor degradation or limited impact | 2 hours | Performance issues, single peer issues |
| **P4 - Low** | Minimal impact or cosmetic issues | 24 hours | UI bugs, non-critical warnings |

## 📋 Incident Response Team

### Roles and Responsibilities

| Role | Primary Responsibilities | Secondary Responsibilities |
|------|-------------------------|---------------------------|
| **Incident Commander (IC)** | Overall incident coordination | External communication |
| **Technical Lead (TL)** | Technical investigation and resolution | Resource allocation |
| **Communications Lead (CL)** | Stakeholder updates | Status page updates |
| **Operations Lead (OL)** | System changes and monitoring | Backup coordination |

### Escalation Chain

```
On-Call Engineer (5 min)
    ↓
Team Lead (15 min)
    ↓
Engineering Manager (30 min)
    ↓
CTO/VP Engineering (1 hour)
```

## 🎯 Incident Response Procedures

### Phase 1: Detection & Alert

1. **Automated Detection**
   - Monitoring alerts trigger
   - Health check failures
   - User reports

2. **Initial Assessment**
   ```bash
   # Quick health check
   curl -f http://localhost:20332/health || echo "RPC not responding"
   
   # Check node status
   systemctl status neo-node
   
   # Recent errors
   journalctl -u neo-node -p err -n 50
   ```

3. **Severity Assignment**
   - Use classification matrix
   - Consider business impact
   - Evaluate data risk

### Phase 2: Triage & Communication

1. **Create Incident Channel**
   ```
   #incident-2025-01-15-node-outage
   ```

2. **Initial Communication**
   ```markdown
   **INCIDENT DECLARED**
   - Time: [timestamp]
   - Severity: P[1-4]
   - Impact: [description]
   - IC: @[name]
   - Status: Investigating
   ```

3. **Gather Initial Data**
   ```bash
   # Run diagnostics
   ./scripts/incident_diagnostics.sh > incident_$(date +%Y%m%d_%H%M%S).log
   ```

### Phase 3: Investigation & Mitigation

Follow specific runbooks based on incident type (see sections below).

### Phase 4: Resolution & Recovery

1. **Verify Resolution**
   ```bash
   # Run validation
   ./scripts/validate_sync.sh
   
   # Check metrics
   curl http://localhost:9090/metrics | grep -E "up|health"
   ```

2. **Monitor Stability**
   - Watch for 30 minutes
   - Check error rates
   - Verify performance

### Phase 5: Post-Incident Review

1. **Document Timeline**
2. **Identify Root Cause**
3. **Create Action Items**
4. **Update Runbooks**

## 🔥 Specific Incident Runbooks

### 1. Node Crash/Unresponsive

**Detection:**
- No response to health checks
- Process not running
- Monitoring alerts

**Immediate Actions:**
```bash
# 1. Check process
ps aux | grep neo-node

# 2. Check for OOM kill
dmesg | grep -i "killed process"

# 3. Check disk space
df -h

# 4. Attempt restart
systemctl restart neo-node

# 5. If fails, check logs
tail -n 1000 /var/log/neo-node/node.log | grep -i error
```

**Resolution Steps:**

If OOM:
```bash
# Increase memory limit
echo "MemoryMax=16G" >> /etc/systemd/system/neo-node.service.d/override.conf
systemctl daemon-reload
systemctl restart neo-node
```

If disk full:
```bash
# Emergency cleanup
./scripts/emergency_cleanup.sh
# Consider moving to larger disk
```

If corruption:
```bash
# Restore from backup
./scripts/restore_backup.sh --latest
```

### 2. Consensus Failure (Validator Nodes)

**Detection:**
- Consensus stalled alerts
- View number not advancing
- High consensus round failures

**Immediate Actions:**
```bash
# 1. Check consensus state
curl http://localhost:20332/consensus/state

# 2. Check other validators
for validator in validator1 validator2 validator3; do
    echo "Checking $validator"
    curl http://$validator:20332/consensus/state
done

# 3. Check network connectivity to validators
./scripts/check_validator_connectivity.sh
```

**Resolution Steps:**

If isolated:
```bash
# Restart consensus
systemctl restart neo-node
```

If widespread:
```bash
# Coordinate with other validators
# May need emergency view change
./scripts/force_view_change.sh --coordinate
```

### 3. Data Corruption

**Detection:**
- Block verification failures
- State root mismatches
- Database errors in logs

**Immediate Actions:**
```bash
# 1. Stop node to prevent propagation
systemctl stop neo-node

# 2. Backup current state
tar -czf corrupted_state_$(date +%Y%m%d_%H%M%S).tar.gz /var/neo/data

# 3. Run integrity check
./scripts/check_db_integrity.sh
```

**Resolution Steps:**

Option 1 - Repair:
```bash
# Attempt repair
./scripts/repair_database.sh
```

Option 2 - Restore:
```bash
# Restore from backup
./scripts/restore_backup.sh --verify
```

Option 3 - Resync:
```bash
# Full resync (last resort)
rm -rf /var/neo/data/chain.db
systemctl start neo-node
```

### 4. Network Attack (DDoS)

**Detection:**
- Abnormally high connection count
- High bandwidth usage
- RPC timeout alerts

**Immediate Actions:**
```bash
# 1. Check connection count
ss -tn state established | grep :20333 | wc -l

# 2. Identify attack pattern
ss -tn state established | grep :20333 | awk '{print $5}' | cut -d: -f1 | sort | uniq -c | sort -rn | head -20

# 3. Enable emergency rate limiting
iptables -I INPUT -p tcp --dport 20333 -m state --state NEW -m recent --set
iptables -I INPUT -p tcp --dport 20333 -m state --state NEW -m recent --update --seconds 60 --hitcount 10 -j DROP
```

**Resolution Steps:**

1. **Block malicious IPs:**
```bash
# Add to blocklist
./scripts/block_ips.sh suspicious_ips.txt
```

2. **Enable DDoS protection:**
```bash
# CloudFlare or similar
./scripts/enable_ddos_protection.sh
```

3. **Scale horizontally:**
```bash
# Deploy additional nodes behind load balancer
./scripts/deploy_node_fleet.sh --count 3
```

### 5. Split Brain Scenario

**Detection:**
- Different block heights on different nodes
- Conflicting block hashes
- Network partition alerts

**Immediate Actions:**
```bash
# 1. Identify partitions
./scripts/check_network_partitions.sh

# 2. Compare states
for node in node1 node2 node3; do
    echo "Node: $node"
    curl http://$node:20332/getblockcount
    curl http://$node:20332/getbestblockhash
done
```

**Resolution Steps:**

1. **Identify authoritative chain:**
```bash
# Usually the partition with more validators
./scripts/identify_canonical_chain.sh
```

2. **Resync minority partition:**
```bash
# On minority nodes
systemctl stop neo-node
rm -rf /var/neo/data/chain.db
systemctl start neo-node
```

### 6. Performance Degradation

**Detection:**
- High response times
- Low sync rate
- Resource exhaustion alerts

**Immediate Actions:**
```bash
# 1. Check resource usage
htop
iostat -x 5 5
sar -n DEV 1 10

# 2. Identify bottlenecks
./scripts/performance_diagnostics.sh

# 3. Check for unusual activity
tail -f /var/log/neo-node/node.log | grep -E "slow|timeout|error"
```

**Resolution Steps:**

If CPU bound:
```bash
# Reduce load
echo "max_peers = 30" >> config.toml
systemctl restart neo-node
```

If I/O bound:
```bash
# Optimize database
./scripts/optimize_database.sh
```

If memory pressure:
```bash
# Clear caches
sync && echo 3 > /proc/sys/vm/drop_caches
```

## 📊 Incident Communication Templates

### Initial Declaration

```markdown
**INCIDENT ALERT**

**Time:** [timestamp]
**Severity:** P[1-4]
**Service:** Neo Node [testnet/mainnet]
**Impact:** [User-facing impact description]

**Current Status:** Investigating

**Next Update:** In 15 minutes

IC: @[name]
```

### Progress Update

```markdown
**INCIDENT UPDATE**

**Time:** [timestamp]
**Severity:** P[1-4] (unchanged/downgraded)

**Progress:**
- [What we know]
- [What we've done]
- [Current status]

**Next Steps:**
- [Planned actions]

**Next Update:** In [15/30/60] minutes
```

### Resolution Notice

```markdown
**INCIDENT RESOLVED**

**Time:** [timestamp]
**Duration:** [total time]
**Root Cause:** [brief description]

**Impact Summary:**
- [Service degradation details]
- [Data impact if any]

**Follow-up Actions:**
- Post-incident review scheduled for [date]
- [Any immediate action items]

Thank you for your patience.
```

## 🛠️ Incident Tools

### Diagnostic Script

Create `/opt/neo-rs/scripts/incident_diagnostics.sh`:

```bash
#!/bin/bash
# Comprehensive diagnostics collection

echo "=== Incident Diagnostics - $(date) ==="

echo -e "\n--- System Status ---"
uptime
free -h
df -h
systemctl status neo-node --no-pager

echo -e "\n--- Network Status ---"
ss -s
ss -tn state established | grep -E ":20333|:20332" | wc -l

echo -e "\n--- Node Metrics ---"
curl -s http://localhost:20332/health
curl -s http://localhost:20332/getblockcount
curl -s http://localhost:20332/getpeers | jq '.result.connected | length'

echo -e "\n--- Recent Errors ---"
journalctl -u neo-node -p err -n 100 --no-pager

echo -e "\n--- Performance Metrics ---"
curl -s http://localhost:9090/metrics | grep -E "process_cpu|process_resident_memory|neo_block_height"

echo -e "\n--- Database Status ---"
du -sh /var/neo/data/*

echo "=== End Diagnostics ==="
```

### Quick Recovery Script

Create `/opt/neo-rs/scripts/quick_recovery.sh`:

```bash
#!/bin/bash
# Automated recovery attempts

echo "Starting quick recovery procedure..."

# 1. Try simple restart
echo "Attempting restart..."
systemctl restart neo-node
sleep 30

if systemctl is-active neo-node; then
    echo "Node restarted successfully"
    exit 0
fi

# 2. Clear temporary files
echo "Clearing temporary files..."
find /var/neo/data -name "*.tmp" -delete
find /var/neo/data -name "LOCK" -delete

# 3. Try again
systemctl start neo-node
sleep 30

if systemctl is-active neo-node; then
    echo "Node started after cleanup"
    exit 0
fi

# 4. Last resort - restore from backup
echo "Restart failed, considering backup restore..."
echo "Run: ./scripts/restore_backup.sh --latest"
exit 1
```

## 📈 Metrics to Monitor During Incidents

### Key Performance Indicators

| Metric | Normal Range | Warning | Critical |
|--------|-------------|---------|----------|
| Block Height Lag | < 10 blocks | > 50 blocks | > 100 blocks |
| RPC Response Time | < 100ms | > 500ms | > 2000ms |
| Peer Count | 5-50 | < 3 | 0 |
| Memory Usage | < 4GB | > 6GB | > 8GB |
| CPU Usage | < 60% | > 80% | > 95% |
| Disk I/O Wait | < 10% | > 30% | > 50% |

### Monitoring Commands

```bash
# Real-time monitoring during incident
watch -n 1 'curl -s http://localhost:20332/getblockcount | jq .result'

# Resource monitoring
dstat -cdnmpy 5

# Connection monitoring
watch -n 5 'ss -s | grep estab'
```

## 📚 Post-Incident Review Template

```markdown
# Post-Incident Review: [Incident ID]

**Date:** [Date of incident]
**Duration:** [Start time] - [End time]
**Severity:** P[1-4]
**Participants:** [List of people involved]

## Timeline
- [Time]: Detection - [How it was detected]
- [Time]: Response began - [First responder actions]
- [Time]: Escalation - [If applicable]
- [Time]: Mitigation - [What was done]
- [Time]: Resolution - [How it was resolved]
- [Time]: All clear - [Verification complete]

## Root Cause Analysis

### What Happened
[Detailed description of the incident]

### Why It Happened
[Root cause analysis - use 5 Whys if helpful]

### Impact
- [User impact]
- [Data impact]
- [Business impact]

## What Went Well
- [Things that worked]

## What Went Wrong
- [Things that didn't work]

## Action Items
| Action | Owner | Due Date | Priority |
|--------|-------|----------|----------|
| [Action 1] | [Name] | [Date] | [High/Med/Low] |

## Lessons Learned
[Key takeaways for the team]
```

## 🔄 Continuous Improvement

1. **Regular Drills**
   - Monthly incident response drills
   - Rotate incident commander role
   - Test communication channels

2. **Runbook Updates**
   - Review after each incident
   - Add new scenarios
   - Update contact information

3. **Automation**
   - Automate common fixes
   - Improve diagnostic collection
   - Enhanced monitoring

---

Remember: During an incident, **stay calm**, **communicate clearly**, and **document everything**. The goal is rapid resolution while preventing recurrence.