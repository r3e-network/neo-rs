# Neo Rust Node Disaster Recovery Procedures

## Overview

This document outlines comprehensive disaster recovery procedures for Neo Rust nodes to ensure rapid recovery from various failure scenarios while maintaining data integrity and minimizing downtime.

## 🚨 Emergency Contact Information

- **Primary On-Call**: [Configure in deployment]
- **Secondary On-Call**: [Configure in deployment]
- **Escalation**: [Configure in deployment]
- **Status Page**: [Configure in deployment]

## 📊 Failure Scenarios and Recovery

### 1. Node Crash / Unresponsive Node

**Symptoms:**
- Node process terminated unexpectedly
- RPC endpoints not responding
- No new blocks being processed

**Recovery Steps:**

```bash
# 1. Check node status
systemctl status neo-node

# 2. Check recent logs for crash reason
journalctl -u neo-node -n 100

# 3. Check system resources
df -h           # Disk space
free -h         # Memory
dmesg | tail    # System errors

# 4. Attempt restart
systemctl restart neo-node

# 5. If restart fails, check for corruption
./scripts/check_db_integrity.sh

# 6. Monitor startup
tail -f /var/log/neo-node/node.log
```

**Automated Recovery Script:**

```bash
#!/bin/bash
# auto_restart.sh - Place in /opt/neo-rs/scripts/

MAX_RETRIES=3
RETRY_DELAY=30

for i in $(seq 1 $MAX_RETRIES); do
    echo "Restart attempt $i of $MAX_RETRIES"
    systemctl restart neo-node
    
    sleep $RETRY_DELAY
    
    if systemctl is-active neo-node > /dev/null; then
        echo "Node successfully restarted"
        exit 0
    fi
done

echo "Failed to restart after $MAX_RETRIES attempts"
# Send alert
./send_alert.sh "Node restart failed after $MAX_RETRIES attempts"
exit 1
```

### 2. Database Corruption

**Symptoms:**
- Startup fails with database errors
- Block verification failures
- State inconsistencies

**Recovery Steps:**

```bash
# 1. Stop the node
systemctl stop neo-node

# 2. Backup corrupted database
tar -czf corrupted_db_$(date +%Y%m%d_%H%M%S).tar.gz /var/neo/data

# 3. Run database repair tool
./scripts/repair_database.sh

# 4. If repair fails, restore from backup
./scripts/restore_from_backup.sh --latest

# 5. If no valid backup, resync from network
rm -rf /var/neo/data/chain.db
systemctl start neo-node
```

**Database Integrity Check Script:**

```bash
#!/bin/bash
# check_db_integrity.sh

DB_PATH="/var/neo/data"
LOG_FILE="/var/log/neo-node/integrity_check.log"

echo "[$(date)] Starting database integrity check" >> $LOG_FILE

# Check RocksDB integrity
rocksdb_ldb --db=$DB_PATH/chain.db checksst 2>&1 | tee -a $LOG_FILE

if [ $? -ne 0 ]; then
    echo "[$(date)] Database corruption detected!" >> $LOG_FILE
    ./send_alert.sh "Database corruption detected on $(hostname)"
    exit 1
fi

echo "[$(date)] Database integrity check passed" >> $LOG_FILE
```

### 3. Network Partition / Loss of Connectivity

**Symptoms:**
- Zero connected peers
- No new blocks received
- Consensus failures (validator nodes)

**Recovery Steps:**

```bash
# 1. Check network connectivity
ping -c 4 8.8.8.8
curl -I https://neo.org

# 2. Check firewall rules
sudo iptables -L -n
sudo ufw status

# 3. Test seed node connectivity
for seed in seed{1..5}t5.neo.org; do
    nc -zv $seed 20333
done

# 4. Reset peer database
systemctl stop neo-node
rm -f /var/neo/data/peers.dat
systemctl start neo-node

# 5. Force peer connections
./scripts/add_peers.sh
```

### 4. Disk Space Exhaustion

**Symptoms:**
- Write errors in logs
- Node stops processing blocks
- Database errors

**Prevention and Recovery:**

```bash
# 1. Emergency space cleanup
#!/bin/bash
# emergency_cleanup.sh

# Remove old logs
find /var/log/neo-node -name "*.log.*" -mtime +7 -delete

# Clear package cache
apt-get clean

# Remove old backups (keep last 3)
ls -t /var/backups/neo-*.tar.gz | tail -n +4 | xargs rm -f

# 2. Expand storage (if possible)
# AWS: Expand EBS volume
# Physical: Add new disk and migrate

# 3. Enable log rotation
cat > /etc/logrotate.d/neo-node << EOF
/var/log/neo-node/*.log {
    daily
    rotate 7
    compress
    delaycompress
    notifempty
    create 0640 neo neo
    sharedscripts
    postrotate
        systemctl reload neo-node
    endscript
}
EOF
```

### 5. Memory Exhaustion / OOM Kill

**Symptoms:**
- Node process killed by OOM killer
- System extremely slow
- Random crashes

**Recovery and Prevention:**

```bash
# 1. Check OOM killer logs
dmesg | grep -i "killed process"
grep -i "out of memory" /var/log/syslog

# 2. Configure memory limits
cat > /etc/systemd/system/neo-node.service.d/memory.conf << EOF
[Service]
MemoryMax=8G
MemoryHigh=7G
EOF

# 3. Enable swap (emergency only)
fallocate -l 4G /swapfile
chmod 600 /swapfile
mkswap /swapfile
swapon /swapfile

# 4. Optimize node configuration
# Reduce cache sizes, connection limits, etc.
```

### 6. Consensus Node Specific Failures

**For Validator Nodes Only**

**Key Loss/Compromise:**

```bash
# IMMEDIATE ACTIONS:
# 1. Stop the node
systemctl stop neo-node

# 2. Notify other validators
./notify_validators.sh "Key compromise suspected"

# 3. Generate new keys (offline)
./generate_consensus_keys.sh

# 4. Update configuration
# 5. Coordinate with other validators for update
```

**Consensus Stall:**

```bash
# 1. Check consensus state
curl http://localhost:20332/consensus

# 2. Check view number progression
watch -n 1 'curl -s http://localhost:20332/consensus | jq .view'

# 3. Force view change (emergency only)
./force_view_change.sh
```

## 🔄 Backup Procedures

### Automated Backup System

Create `/opt/neo-rs/scripts/backup_node.sh`:

```bash
#!/bin/bash
# Automated backup script

BACKUP_DIR="/var/backups/neo"
RETENTION_DAYS=7
S3_BUCKET="neo-backups"  # Optional S3 backup

# Create backup
backup_node() {
    local timestamp=$(date +%Y%m%d_%H%M%S)
    local backup_file="neo_backup_${timestamp}.tar.gz"
    
    echo "[$(date)] Starting backup to $backup_file"
    
    # Stop writes (optional for consistency)
    curl -X POST http://localhost:20332/maintenance/enable
    
    # Create backup
    tar -czf "$BACKUP_DIR/$backup_file" \
        --exclude='*.log' \
        --exclude='peers.dat' \
        /var/neo/data
    
    # Resume writes
    curl -X POST http://localhost:20332/maintenance/disable
    
    # Upload to S3 (if configured)
    if [ -n "$S3_BUCKET" ]; then
        aws s3 cp "$BACKUP_DIR/$backup_file" "s3://$S3_BUCKET/"
    fi
    
    # Clean old backups
    find $BACKUP_DIR -name "neo_backup_*.tar.gz" -mtime +$RETENTION_DAYS -delete
    
    echo "[$(date)] Backup completed: $backup_file"
}

# Run backup
backup_node
```

### Backup Schedule

Add to crontab:

```bash
# Daily backup at 3 AM
0 3 * * * /opt/neo-rs/scripts/backup_node.sh >> /var/log/neo-backup.log 2>&1

# Hourly state snapshot (lightweight)
0 * * * * /opt/neo-rs/scripts/snapshot_state.sh
```

## 🚀 Rapid Recovery Procedures

### 1. Hot Standby Failover

```bash
#!/bin/bash
# failover_to_standby.sh

STANDBY_NODE="standby.example.com"
PRIMARY_NODE="primary.example.com"

# 1. Verify standby is ready
ssh $STANDBY_NODE "systemctl is-active neo-node"

# 2. Stop primary (if accessible)
ssh $PRIMARY_NODE "systemctl stop neo-node" || true

# 3. Update DNS/Load balancer
./update_dns.sh $STANDBY_NODE

# 4. Promote standby
ssh $STANDBY_NODE "/opt/neo-rs/scripts/promote_to_primary.sh"

# 5. Verify operation
sleep 10
curl http://$STANDBY_NODE:20332/health
```

### 2. Point-in-Time Recovery

```bash
#!/bin/bash
# restore_to_point.sh

TARGET_HEIGHT=$1
BACKUP_DIR="/var/backups/neo"

# 1. Find appropriate backup
BACKUP=$(find $BACKUP_DIR -name "*.tar.gz" | sort | tail -1)

# 2. Restore backup
systemctl stop neo-node
tar -xzf $BACKUP -C /

# 3. Rollback to target height
./rollback_to_height.sh $TARGET_HEIGHT

# 4. Start node
systemctl start neo-node
```

## 📋 Recovery Validation

### Post-Recovery Checklist

```bash
#!/bin/bash
# validate_recovery.sh

echo "=== Neo Node Recovery Validation ==="

# 1. Process running
if systemctl is-active neo-node > /dev/null; then
    echo "✓ Node process running"
else
    echo "✗ Node process not running"
    exit 1
fi

# 2. RPC responding
if curl -s http://localhost:20332/health > /dev/null; then
    echo "✓ RPC endpoint responding"
else
    echo "✗ RPC endpoint not responding"
fi

# 3. Syncing blocks
HEIGHT1=$(curl -s http://localhost:20332 -d '{"jsonrpc":"2.0","method":"getblockcount","params":[],"id":1}' | jq -r .result)
sleep 30
HEIGHT2=$(curl -s http://localhost:20332 -d '{"jsonrpc":"2.0","method":"getblockcount","params":[],"id":1}' | jq -r .result)

if [ "$HEIGHT2" -gt "$HEIGHT1" ]; then
    echo "✓ Node syncing blocks"
else
    echo "✗ Node not syncing"
fi

# 4. Peer connections
PEERS=$(curl -s http://localhost:20332 -d '{"jsonrpc":"2.0","method":"getpeers","params":[],"id":1}' | jq -r '.result.connected | length')
if [ "$PEERS" -gt 0 ]; then
    echo "✓ Connected to $PEERS peers"
else
    echo "✗ No peer connections"
fi

# 5. Run full validation
./scripts/validate_sync.sh
```

## 🎯 Incident Response Workflow

### Severity Levels

- **P1 (Critical)**: Complete service outage, data loss risk
- **P2 (High)**: Degraded service, consensus issues
- **P3 (Medium)**: Performance issues, non-critical failures
- **P4 (Low)**: Minor issues, cosmetic problems

### Response Timeline

| Severity | Initial Response | Resolution Target |
|----------|-----------------|-------------------|
| P1 | 15 minutes | 2 hours |
| P2 | 30 minutes | 4 hours |
| P3 | 2 hours | 24 hours |
| P4 | 24 hours | 72 hours |

### Incident Commander Checklist

1. **Assess** - Determine severity and impact
2. **Notify** - Alert stakeholders and team
3. **Isolate** - Prevent cascade failures
4. **Diagnose** - Identify root cause
5. **Recover** - Execute recovery procedure
6. **Validate** - Confirm full recovery
7. **Document** - Create incident report

## 📊 Monitoring During Recovery

```bash
#!/bin/bash
# recovery_monitor.sh

while true; do
    clear
    echo "=== Recovery Monitor - $(date) ==="
    
    # Node status
    echo -n "Node Status: "
    systemctl is-active neo-node
    
    # Block height
    HEIGHT=$(curl -s http://localhost:20332 -d '{"jsonrpc":"2.0","method":"getblockcount","params":[],"id":1}' | jq -r .result)
    echo "Block Height: $HEIGHT"
    
    # Peer count
    PEERS=$(curl -s http://localhost:20332 -d '{"jsonrpc":"2.0","method":"getpeers","params":[],"id":1}' | jq -r '.result.connected | length')
    echo "Connected Peers: $PEERS"
    
    # Memory usage
    echo "Memory Usage:"
    ps aux | grep neo-node | grep -v grep | awk '{print $4"%"}'
    
    # Disk usage
    echo "Disk Usage:"
    df -h | grep "/var/neo"
    
    sleep 5
done
```

## 🔐 Security Considerations During Recovery

1. **Verify Backups** - Check integrity before restore
2. **Secure Communications** - Use encrypted channels
3. **Access Control** - Limit recovery access
4. **Audit Trail** - Log all recovery actions
5. **Key Management** - Never expose private keys

## 📚 Documentation and Training

1. **Runbooks** - Maintain detailed procedures
2. **Regular Drills** - Practice recovery quarterly
3. **Update Procedures** - Review after incidents
4. **Knowledge Transfer** - Train all team members

---

**Remember**: During an incident, stay calm, follow procedures, and communicate clearly. When in doubt, prioritize data integrity over availability.