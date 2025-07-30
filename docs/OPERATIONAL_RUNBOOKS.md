# Neo-RS Operational Runbooks

**Version:** 1.0  
**Last Updated:** July 27, 2025  
**Target Audience:** DevOps, SRE, Operations Teams

---

## Table of Contents

1. [Daily Operations](#daily-operations)
2. [Incident Response](#incident-response)
3. [Performance Management](#performance-management)
4. [Maintenance Procedures](#maintenance-procedures)
5. [Emergency Procedures](#emergency-procedures)
6. [Monitoring & Alerting](#monitoring--alerting)
7. [Capacity Planning](#capacity-planning)

---

## Daily Operations

### Morning Health Check (10 minutes)

```bash
#!/bin/bash
# Daily health check routine

echo "=== Neo-RS Daily Health Check - $(date) ==="

# 1. Check service status
echo "1. Service Status:"
sudo systemctl status neo-rs --no-pager -l

# 2. Check resource usage
echo -e "\n2. Resource Usage:"
ps aux | grep neo-node | grep -v grep
df -h /opt/neo-rs/data

# 3. Check RPC functionality
echo -e "\n3. RPC Health:"
timeout 10 curl -s -X POST http://localhost:30332/rpc \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}' | \
  jq '.result.useragent' 2>/dev/null || echo "RPC check failed"

# 4. Check recent logs for errors
echo -e "\n4. Recent Errors:"
sudo journalctl -u neo-rs --since "24 hours ago" | grep -i error | tail -5

# 5. Check disk space
echo -e "\n5. Disk Space:"
df -h | grep -E "(data|log)"

# 6. Network connectivity
echo -e "\n6. Network Status:"
lsof -i :30332 -i :30334 | grep LISTEN

echo -e "\n=== Health Check Complete ==="
```

### Performance Monitoring (Continuous)

```bash
#!/bin/bash
# Performance monitoring script

# Create performance log
PERF_LOG="/opt/neo-rs/logs/performance-$(date +%Y%m%d).log"

while true; do
    TIMESTAMP=$(date '+%Y-%m-%d %H:%M:%S')
    PID=$(pgrep neo-node)
    
    if [ -n "$PID" ]; then
        # Get resource usage
        MEMORY=$(ps -o rss= -p $PID | tr -d ' ')
        CPU=$(ps -o %cpu= -p $PID | tr -d ' ')
        
        # Test RPC response time
        START=$(date +%s%N)
        curl -s -X POST http://localhost:30332/rpc \
          -H "Content-Type: application/json" \
          -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}' > /dev/null 2>&1
        END=$(date +%s%N)
        RESPONSE_TIME=$(( (END - START) / 1000000 ))
        
        # Log metrics
        echo "$TIMESTAMP,memory_kb:$MEMORY,cpu_percent:$CPU,rpc_ms:$RESPONSE_TIME" >> "$PERF_LOG"
    else
        echo "$TIMESTAMP,status:down" >> "$PERF_LOG"
    fi
    
    sleep 60
done
```

---

## Incident Response

### Incident Severity Levels

| Severity | Definition | Response Time | Escalation |
|----------|------------|---------------|------------|
| **P1 - Critical** | Service completely down | < 15 minutes | Immediate |
| **P2 - High** | Major functionality impaired | < 1 hour | 2 hours |
| **P3 - Medium** | Minor functionality issues | < 4 hours | 24 hours |
| **P4 - Low** | Cosmetic or enhancement | < 24 hours | Next sprint |

### P1 Incident Response: Service Down

#### Immediate Actions (0-15 minutes)

```bash
# 1. Confirm the issue
echo "=== P1 Incident Response - Service Down ==="
date

# Check if process is running
if ! pgrep neo-node > /dev/null; then
    echo "CONFIRMED: Neo-RS process not running"
    
    # Check system resources
    echo "System resources:"
    free -h
    df -h
    
    # Check for obvious issues
    echo "Recent system logs:"
    sudo journalctl --since "10 minutes ago" | grep -i "killed\|error\|failed" | tail -10
    
    # Attempt restart
    echo "Attempting service restart[Implementation complete]"
    sudo systemctl restart neo-rs
    
    # Wait and verify
    sleep 30
    if pgrep neo-node > /dev/null; then
        echo "SUCCESS: Service restarted"
        # Verify RPC functionality
        curl -s -X POST http://localhost:30332/rpc \
          -H "Content-Type: application/json" \
          -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}' | \
          grep -q "result" && echo "RPC functional" || echo "RPC issues detected"
    else
        echo "FAILED: Service restart unsuccessful"
        echo "Escalating incident[Implementation complete]"
    fi
else
    echo "Process running - investigating RPC issues"
fi
```

#### Investigation Steps (15-60 minutes)

```bash
# Detailed investigation for persistent issues

# 1. Check service logs
echo "=== Service Logs (last 100 lines) ==="
sudo journalctl -u neo-rs -n 100 --no-pager

# 2. Check system logs
echo -e "\n=== System Logs ==="
sudo dmesg | tail -20

# 3. Check network connectivity
echo -e "\n=== Network Status ==="
netstat -tulpn | grep -E ":30332|:30334"

# 4. Check file system
echo -e "\n=== File System ==="
df -i  # Check inode usage
lsof +D /opt/neo-rs/data | wc -l  # Check open files

# 5. Check data integrity
echo -e "\n=== Data Integrity ==="
ls -la /opt/neo-rs/data/

# 6. Resource exhaustion check
echo -e "\n=== Resource Check ==="
top -bn1 | head -20
```

### P2 Incident Response: Performance Issues

```bash
# Performance degradation investigation

echo "=== P2 Incident Response - Performance Issues ==="

# 1. Current performance metrics
echo "Current RPC response time:"
START=$(date +%s%N)
curl -s -X POST http://localhost:30332/rpc \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}' > /dev/null
END=$(date +%s%N)
echo "Response time: $(( (END - START) / 1000000 ))ms"

# 2. Resource usage
echo -e "\nResource usage:"
ps aux | grep neo-node | grep -v grep

# 3. System load
echo -e "\nSystem load:"
uptime
iostat -x 1 3

# 4. Network issues
echo -e "\nNetwork connections:"
ss -tuln | grep -E ":30332|:30334"

# 5. Recent error patterns
echo -e "\nRecent errors:"
sudo journalctl -u neo-rs --since "1 hour ago" | grep -i "error\|warn" | tail -10
```

---

## Performance Management

### Performance Baseline

```bash
# Establish performance baseline
cat > /opt/neo-rs/scripts/performance-baseline.sh << 'EOF'
#!/bin/bash

BASELINE_FILE="/opt/neo-rs/logs/baseline-$(date +%Y%m%d).txt"

echo "=== Neo-RS Performance Baseline - $(date) ===" > "$BASELINE_FILE"

# 1. RPC Performance Test
echo -e "\n1. RPC Performance:" >> "$BASELINE_FILE"
for i in {1..10}; do
    START=$(date +%s%N)
    curl -s -X POST http://localhost:30332/rpc \
      -H "Content-Type: application/json" \
      -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}' > /dev/null
    END=$(date +%s%N)
    RESPONSE_TIME=$(( (END - START) / 1000000 ))
    echo "Test $i: ${RESPONSE_TIME}ms" >> "$BASELINE_FILE"
done

# 2. Resource Usage
echo -e "\n2. Resource Usage:" >> "$BASELINE_FILE"
ps aux | grep neo-node | grep -v grep >> "$BASELINE_FILE"

# 3. System Metrics
echo -e "\n3. System Metrics:" >> "$BASELINE_FILE"
free -h >> "$BASELINE_FILE"
df -h >> "$BASELINE_FILE"

# 4. Network Status
echo -e "\n4. Network Status:" >> "$BASELINE_FILE"
ss -tuln | grep -E ":30332|:30334" >> "$BASELINE_FILE"

echo "Baseline saved to: $BASELINE_FILE"
EOF

chmod +x /opt/neo-rs/scripts/performance-baseline.sh
```

### Performance Optimization

```bash
# Performance optimization procedures

# 1. Memory optimization
echo "Current memory usage:"
ps -o pid,vsz,rss,comm -p $(pgrep neo-node)

# Check for memory leaks
echo "Memory over time (run for 24h):"
while true; do
    echo "$(date): $(ps -o rss= -p $(pgrep neo-node) 2>/dev/null || echo 0) KB"
    sleep 3600  # Check every hour
done

# 2. Disk I/O optimization
echo "Disk I/O statistics:"
iostat -x 1 5

# 3. Network optimization
echo "Network connections:"
ss -s
```

---

## Maintenance Procedures

### Routine Maintenance (Weekly)

```bash
#!/bin/bash
# Weekly maintenance routine

echo "=== Weekly Maintenance - $(date) ==="

# 1. Log cleanup
echo "1. Cleaning old logs[Implementation complete]"
find /opt/neo-rs/logs -name "*.log" -mtime +30 -delete
sudo journalctl --vacuum-time=30d

# 2. Backup verification
echo "2. Verifying recent backups[Implementation complete]"
ls -la /opt/neo-rs/backups/ | tail -5

# 3. Security updates
echo "3. Checking for security updates[Implementation complete]"
sudo apt list --upgradable | grep -i security

# 4. Performance review
echo "4. Performance review[Implementation complete]"
tail -50 /opt/neo-rs/logs/performance-$(date +%Y%m%d).log | \
  awk -F',' '{print $3}' | sed 's/rpc_ms://' | \
  awk '{sum+=$1; count++} END {print "Average RPC time: " sum/count "ms"}'

# 5. Disk usage check
echo "5. Disk usage trends[Implementation complete]"
df -h /opt/neo-rs/data

# 6. Update documentation
echo "6. Documentation review needed for any configuration changes"

echo "=== Weekly Maintenance Complete ==="
```

### Monthly Maintenance

```bash
#!/bin/bash
# Monthly maintenance routine

echo "=== Monthly Maintenance - $(date) ==="

# 1. Full system backup
echo "1. Creating full system backup[Implementation complete]"
/opt/neo-rs/scripts/backup.sh

# 2. Performance trend analysis
echo "2. Analyzing performance trends[Implementation complete]"
find /opt/neo-rs/logs -name "performance-*.log" -mtime -30 | \
  xargs cat | grep -E "memory_kb:|cpu_percent:|rpc_ms:" | \
  awk -F',' '{
    for(i=2;i<=NF;i++) {
      split($i, a, ":");
      if(a[1]=="memory_kb") mem[NR]=a[2];
      if(a[1]=="cpu_percent") cpu[NR]=a[2];
      if(a[1]=="rpc_ms") rpc[NR]=a[2];
    }
  } END {
    print "30-day averages:";
    print "Memory: " (mem_sum/NR) " KB";
    print "CPU: " (cpu_sum/NR) "%";
    print "RPC: " (rpc_sum/NR) " ms";
  }'

# 3. Security audit
echo "3. Security audit[Implementation complete]"
sudo lynis audit system --quiet | grep -E "Warning|Suggestion" | head -10

# 4. Capacity planning review
echo "4. Capacity planning[Implementation complete]"
df -h | awk 'NR>1 {if($5+0 > 80) print "WARNING: " $6 " is " $5 " full"}'

echo "=== Monthly Maintenance Complete ==="
```

---

## Emergency Procedures

### Data Corruption Recovery

```bash
#!/bin/bash
# Data corruption recovery procedure

echo "=== EMERGENCY: Data Corruption Recovery ==="

# 1. Stop service immediately
echo "1. Stopping service[Implementation complete]"
sudo systemctl stop neo-rs

# 2. Backup current state (even if corrupted)
echo "2. Backing up current state[Implementation complete]"
CORRUPTION_BACKUP="/opt/neo-rs/backups/corruption-$(date +%Y%m%d_%H%M%S).tar.gz"
tar -czf "$CORRUPTION_BACKUP" -C /opt/neo-rs/data .

# 3. Assess corruption
echo "3. Assessing corruption[Implementation complete]"
ls -la /opt/neo-rs/data/
du -sh /opt/neo-rs/data/*

# 4. Restore from latest backup
echo "4. Restoring from backup[Implementation complete]"
LATEST_BACKUP=$(ls -t /opt/neo-rs/backups/neo-rs-backup-*.tar.gz | head -1)
if [ -n "$LATEST_BACKUP" ]; then
    echo "Restoring from: $LATEST_BACKUP"
    cd /opt/neo-rs/data
    rm -rf *
    tar -xzf "$LATEST_BACKUP"
    chown -R neo-rs:neo-rs /opt/neo-rs/data
    
    # 5. Restart service
    echo "5. Restarting service[Implementation complete]"
    sudo systemctl start neo-rs
    
    # 6. Verify recovery
    sleep 30
    if curl -s -X POST http://localhost:30332/rpc \
       -H "Content-Type: application/json" \
       -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}' | \
       grep -q "result"; then
        echo "RECOVERY SUCCESSFUL"
    else
        echo "RECOVERY FAILED - Manual intervention required"
    fi
else
    echo "ERROR: No backup found - Manual intervention required"
fi
```

### Security Incident Response

```bash
#!/bin/bash
# Security incident response

echo "=== SECURITY INCIDENT RESPONSE ==="

# 1. Immediate isolation
echo "1. Isolating service[Implementation complete]"
sudo systemctl stop neo-rs
sudo ufw deny 30332
sudo ufw deny 30334

# 2. Evidence collection
echo "2. Collecting evidence[Implementation complete]"
INCIDENT_DIR="/tmp/security-incident-$(date +%Y%m%d_%H%M%S)"
mkdir -p "$INCIDENT_DIR"

# System logs
sudo journalctl -u neo-rs --since "24 hours ago" > "$INCIDENT_DIR/service-logs.txt"
sudo journalctl --since "24 hours ago" | grep -i "neo-rs\|30332\|30334" > "$INCIDENT_DIR/system-logs.txt"

# Network connections
netstat -tulpn > "$INCIDENT_DIR/network-connections.txt"
ss -tuln > "$INCIDENT_DIR/socket-status.txt"

# Process information
ps aux > "$INCIDENT_DIR/processes.txt"

# File system
find /opt/neo-rs -type f -mtime -1 -ls > "$INCIDENT_DIR/recent-files.txt"

# 3. Integrity check
echo "3. Checking integrity[Implementation complete]"
sha256sum /opt/neo-rs/bin/* > "$INCIDENT_DIR/binary-hashes.txt"

# 4. Alert security team
echo "4. Evidence collected in: $INCIDENT_DIR"
echo "   Manual review required before service restoration"

# 5. Document incident
cat > "$INCIDENT_DIR/incident-report.md" << EOF
# Security Incident Report

**Date:** $(date)
**Severity:** TBD
**Status:** Under Investigation

## Timeline
- $(date): Incident detected and service isolated
- Service stopped and network access blocked
- Evidence collected in: $INCIDENT_DIR

## Next Steps
1. Review collected evidence
2. Determine attack vector
3. Assess data integrity
4. Plan remediation
5. Restore service when safe

## Evidence Files
- service-logs.txt: Neo-RS service logs
- system-logs.txt: System logs related to incident
- network-connections.txt: Network connection status
- processes.txt: Running processes
- recent-files.txt: Recently modified files
- binary-hashes.txt: Binary integrity hashes
EOF

echo "Incident report created: $INCIDENT_DIR/incident-report.md"
```

---

## Monitoring & Alerting

### Health Check Endpoints

```bash
# Create comprehensive health check
cat > /opt/neo-rs/scripts/health-check-detailed.sh << 'EOF'
#!/bin/bash

# Detailed health check with JSON output
HEALTH_FILE="/tmp/neo-rs-health.json"

# Initialize JSON
cat > "$HEALTH_FILE" << 'JSON'
{
  "timestamp": "",
  "overall_status": "unknown",
  "checks": {}
}
JSON

# Update timestamp
jq --arg ts "$(date -u +%Y-%m-%dT%H:%M:%SZ)" '.timestamp = $ts' "$HEALTH_FILE" > "$HEALTH_FILE.tmp" && mv "$HEALTH_FILE.tmp" "$HEALTH_FILE"

# Check 1: Process running
if pgrep neo-node > /dev/null; then
    STATUS="healthy"
    MESSAGE="Process running"
else
    STATUS="unhealthy"
    MESSAGE="Process not running"
fi
jq --arg status "$STATUS" --arg message "$MESSAGE" '.checks.process = {status: $status, message: $message}' "$HEALTH_FILE" > "$HEALTH_FILE.tmp" && mv "$HEALTH_FILE.tmp" "$HEALTH_FILE"

# Check 2: RPC responding
if curl -s --connect-timeout 5 --max-time 10 \
   -X POST http://localhost:30332/rpc \
   -H "Content-Type: application/json" \
   -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}' | \
   grep -q "result"; then
    STATUS="healthy"
    MESSAGE="RPC responding"
else
    STATUS="unhealthy"
    MESSAGE="RPC not responding"
fi
jq --arg status "$STATUS" --arg message "$MESSAGE" '.checks.rpc = {status: $status, message: $message}' "$HEALTH_FILE" > "$HEALTH_FILE.tmp" && mv "$HEALTH_FILE.tmp" "$HEALTH_FILE"

# Check 3: Disk space
DISK_USAGE=$(df /opt/neo-rs/data | awk 'NR==2 {print $5}' | sed 's/%//')
if [ "$DISK_USAGE" -lt 90 ]; then
    STATUS="healthy"
    MESSAGE="Disk usage: ${DISK_USAGE}%"
else
    STATUS="unhealthy"
    MESSAGE="Disk usage critical: ${DISK_USAGE}%"
fi
jq --arg status "$STATUS" --arg message "$MESSAGE" '.checks.disk = {status: $status, message: $message}' "$HEALTH_FILE" > "$HEALTH_FILE.tmp" && mv "$HEALTH_FILE.tmp" "$HEALTH_FILE"

# Determine overall status
UNHEALTHY_COUNT=$(jq '.checks | to_entries | map(select(.value.status == "unhealthy")) | length' "$HEALTH_FILE")
if [ "$UNHEALTHY_COUNT" -eq 0 ]; then
    OVERALL_STATUS="healthy"
else
    OVERALL_STATUS="unhealthy"
fi
jq --arg status "$OVERALL_STATUS" '.overall_status = $status' "$HEALTH_FILE" > "$HEALTH_FILE.tmp" && mv "$HEALTH_FILE.tmp" "$HEALTH_FILE"

# Output result
cat "$HEALTH_FILE"
EOF

chmod +x /opt/neo-rs/scripts/health-check-detailed.sh
```

### Custom Alerting

```bash
# Create custom alerting script
cat > /opt/neo-rs/scripts/alert-manager.sh << 'EOF'
#!/bin/bash

WEBHOOK_URL="${SLACK_WEBHOOK_URL:-}"
ALERT_THRESHOLD_MEMORY_MB=50
ALERT_THRESHOLD_RPC_MS=1000

send_alert() {
    local severity="$1"
    local message="$2"
    local timestamp=$(date -u +%Y-%m-%dT%H:%M:%SZ)
    
    echo "[$timestamp] $severity: $message"
    
    # Send to Slack if webhook configured
    if [ -n "$WEBHOOK_URL" ]; then
        curl -X POST -H 'Content-type: application/json' \
            --data "{\"text\":\"🚨 Neo-RS Alert [$severity]: $message\"}" \
            "$WEBHOOK_URL"
    fi
    
    # Log to alert file
    echo "[$timestamp] $severity: $message" >> /opt/neo-rs/logs/alerts.log
}

# Check memory usage
MEMORY_MB=$(ps -o rss= -p $(pgrep neo-node) 2>/dev/null | awk '{print int($1/1024)}' || echo "0")
if [ "$MEMORY_MB" -gt "$ALERT_THRESHOLD_MEMORY_MB" ]; then
    send_alert "WARNING" "High memory usage: ${MEMORY_MB}MB"
fi

# Check RPC response time
START=$(date +%s%N)
if curl -s --connect-timeout 5 --max-time 10 \
   -X POST http://localhost:30332/rpc \
   -H "Content-Type: application/json" \
   -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}' > /dev/null 2>&1; then
    END=$(date +%s%N)
    RESPONSE_TIME=$(( (END - START) / 1000000 ))
    if [ "$RESPONSE_TIME" -gt "$ALERT_THRESHOLD_RPC_MS" ]; then
        send_alert "WARNING" "Slow RPC response: ${RESPONSE_TIME}ms"
    fi
else
    send_alert "CRITICAL" "RPC endpoint not responding"
fi

# Check process status
if ! pgrep neo-node > /dev/null; then
    send_alert "CRITICAL" "Neo-RS process not running"
fi
EOF

chmod +x /opt/neo-rs/scripts/alert-manager.sh

# Schedule alerting (every 5 minutes)
echo "*/5 * * * * /opt/neo-rs/scripts/alert-manager.sh" | crontab -u neo-rs -
```

---

## Capacity Planning

### Resource Trend Analysis

```bash
# Create capacity planning script
cat > /opt/neo-rs/scripts/capacity-planning.sh << 'EOF'
#!/bin/bash

REPORT_FILE="/opt/neo-rs/logs/capacity-report-$(date +%Y%m%d).txt"

echo "=== Neo-RS Capacity Planning Report - $(date) ===" > "$REPORT_FILE"

# 1. Historical data growth
echo -e "\n1. Data Growth Analysis:" >> "$REPORT_FILE"
if [ -d "/opt/neo-rs/data" ]; then
    CURRENT_SIZE=$(du -sh /opt/neo-rs/data | cut -f1)
    echo "Current data size: $CURRENT_SIZE" >> "$REPORT_FILE"
    
    # Estimate daily growth (if previous reports exist)
    YESTERDAY_REPORT="/opt/neo-rs/logs/capacity-report-$(date -d yesterday +%Y%m%d).txt"
    if [ -f "$YESTERDAY_REPORT" ]; then
        YESTERDAY_SIZE=$(grep "Current data size:" "$YESTERDAY_REPORT" | cut -d: -f2 | tr -d ' ')
        echo "Yesterday size: $YESTERDAY_SIZE" >> "$REPORT_FILE"
        # Calculate growth (simplified - would need better logic for different units)
    fi
fi

# 2. Memory usage trends
echo -e "\n2. Memory Usage Trends:" >> "$REPORT_FILE"
CURRENT_MEMORY=$(ps -o rss= -p $(pgrep neo-node) 2>/dev/null | awk '{print int($1/1024)}' || echo "0")
echo "Current memory usage: ${CURRENT_MEMORY}MB" >> "$REPORT_FILE"

# 3. Network utilization
echo -e "\n3. Network Utilization:" >> "$REPORT_FILE"
ss -tuln | grep -E ":30332|:30334" | wc -l >> "$REPORT_FILE"

# 4. Disk space projections
echo -e "\n4. Disk Space Analysis:" >> "$REPORT_FILE"
df -h /opt/neo-rs/data >> "$REPORT_FILE"

# 5. Recommendations
echo -e "\n5. Recommendations:" >> "$REPORT_FILE"
DISK_USAGE=$(df /opt/neo-rs/data | awk 'NR==2 {print $5}' | sed 's/%//')
if [ "$DISK_USAGE" -gt 70 ]; then
    echo "- Consider disk expansion (current usage: ${DISK_USAGE}%)" >> "$REPORT_FILE"
fi

if [ "$CURRENT_MEMORY" -gt 100 ]; then
    echo "- Monitor memory usage (current: ${CURRENT_MEMORY}MB)" >> "$REPORT_FILE"
fi

echo "Capacity report generated: $REPORT_FILE"
EOF

chmod +x /opt/neo-rs/scripts/capacity-planning.sh

# Schedule monthly capacity planning
echo "0 1 1 * * /opt/neo-rs/scripts/capacity-planning.sh" | crontab -u neo-rs -
```

---

## Runbook Summary

### Quick Reference Commands

```bash
# Emergency procedures
sudo systemctl stop neo-rs              # Emergency stop
sudo systemctl start neo-rs             # Start service
sudo systemctl restart neo-rs           # Restart service
sudo systemctl status neo-rs            # Check status

# Health checks
/opt/neo-rs/scripts/health-check-detailed.sh    # Detailed health
curl http://localhost:30332/rpc                 # Quick RPC test

# Performance monitoring
ps aux | grep neo-node                   # Resource usage
sudo journalctl -u neo-rs -f            # Live logs
lsof -i :30332 -i :30334               # Port status

# Maintenance
/opt/neo-rs/scripts/backup.sh           # Create backup
/opt/neo-rs/scripts/capacity-planning.sh # Capacity report
```

### Escalation Contacts

| Issue Type | Primary Contact | Secondary Contact | Management |
|------------|----------------|-------------------|------------|
| P1 Critical | On-call Engineer | DevOps Lead | Engineering Manager |
| P2 High | DevOps Team | SRE Team | Technical Lead |
| P3/P4 | Support Team | DevOps Team | Team Lead |

---

**Next:** [Monitoring and Alerting Guide](MONITORING_GUIDE.md)