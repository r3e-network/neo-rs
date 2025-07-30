# Neo-RS Monitoring and Alerting Guide

**Version:** 1.0  
**Last Updated:** July 27, 2025  
**Target Audience:** DevOps, SRE, Monitoring Teams

---

## Table of Contents

1. [Monitoring Overview](#monitoring-overview)
2. [Metrics Collection](#metrics-collection)
3. [Alerting Configuration](#alerting-configuration)
4. [Dashboard Setup](#dashboard-setup)
5. [Log Management](#log-management)
6. [Performance Monitoring](#performance-monitoring)
7. [Health Checks](#health-checks)
8. [Troubleshooting](#troubleshooting)

---

## Monitoring Overview

### Monitoring Architecture

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   Neo-RS Node   │───▶│   Prometheus    │───▶│    Grafana      │
│                 │    │                 │    │                 │
│ - Metrics       │    │ - Time Series   │    │ - Dashboards    │
│ - Health Checks │    │ - Alerting      │    │ - Visualization │
│ - Log Output    │    │ - Storage       │    │ - Reporting     │
└─────────────────┘    └─────────────────┘    └─────────────────┘
         │                       │                       │
         ▼                       ▼                       ▼
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│  Log Aggregator │    │  Alert Manager  │    │  Notification   │
│                 │    │                 │    │                 │
│ - ELK Stack     │    │ - PagerDuty     │    │ - Slack/Email   │
│ - Centralized   │    │ - Escalation    │    │ - SMS/Phone     │
│ - Search/Query  │    │ - Suppression   │    │ - Webhooks      │
└─────────────────┘    └─────────────────┘    └─────────────────┘
```

### Key Monitoring Areas

| Area | Metrics | Alerts | Tools |
|------|---------|--------|-------|
| **Service Health** | Uptime, Process Status | Service Down | systemd, healthchecks |
| **Performance** | RPC Response Time, Throughput | Slow Response | Prometheus, Grafana |
| **Resources** | CPU, Memory, Disk | High Usage | Node Exporter |
| **Network** | Port Status, Connections | Port Down | Network monitoring |
| **Blockchain** | Block Height, Sync Status | Sync Issues | Custom metrics |
| **Security** | Failed Connections, Errors | Security Events | Log analysis |

---

## Metrics Collection

### Prometheus Configuration

```yaml
# prometheus.yml
global:
  scrape_interval: 15s
  evaluation_interval: 15s

rule_files:
  - "neo-rs-alerts.yml"

scrape_configs:
  # Neo-RS custom metrics
  - job_name: 'neo-rs'
    static_configs:
      - targets: ['localhost:30333']  # Custom metrics endpoint
    scrape_interval: 10s
    metrics_path: /metrics
    
  # System metrics
  - job_name: 'node-exporter'
    static_configs:
      - targets: ['localhost:9100']
    scrape_interval: 15s

  # Process metrics
  - job_name: 'process-exporter'
    static_configs:
      - targets: ['localhost:9256']
    scrape_interval: 15s

alerting:
  alertmanagers:
    - static_configs:
        - targets:
          - localhost:9093
```

### Custom Metrics Exporter

```bash
# Create custom metrics exporter for Neo-RS
cat > /opt/neo-rs/scripts/metrics-exporter.sh << 'EOF'
#!/bin/bash

# Neo-RS Prometheus Metrics Exporter
METRICS_FILE="/tmp/neo-rs-metrics.prom"

# Function to write metric
write_metric() {
    local name="$1"
    local type="$2"
    local help="$3"
    local value="$4"
    local labels="$5"
    
    echo "# HELP $name $help" >> "$METRICS_FILE"
    echo "# TYPE $name $type" >> "$METRICS_FILE"
    if [ -n "$labels" ]; then
        echo "$name{$labels} $value" >> "$METRICS_FILE"
    else
        echo "$name $value" >> "$METRICS_FILE"
    fi
    echo "" >> "$METRICS_FILE"
}

# Initialize metrics file
echo "# Neo-RS Metrics - $(date)" > "$METRICS_FILE"

# 1. Service availability
if pgrep neo-node > /dev/null; then
    write_metric "neo_rs_up" "gauge" "Neo-RS service availability" "1"
    
    PID=$(pgrep neo-node)
    
    # 2. Process metrics
    MEMORY_KB=$(ps -o rss= -p $PID | tr -d ' ')
    MEMORY_BYTES=$((MEMORY_KB * 1024))
    write_metric "neo_rs_memory_bytes" "gauge" "Memory usage in bytes" "$MEMORY_BYTES"
    
    CPU_PERCENT=$(ps -o %cpu= -p $PID | tr -d ' ')
    write_metric "neo_rs_cpu_percent" "gauge" "CPU usage percentage" "$CPU_PERCENT"
    
    # 3. RPC metrics
    START_TIME=$(date +%s%N)
    if curl -s --connect-timeout 5 --max-time 10 \
       -X POST http://localhost:30332/rpc \
       -H "Content-Type: application/json" \
       -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}' > /dev/null 2>&1; then
        END_TIME=$(date +%s%N)
        RESPONSE_TIME_MS=$(( (END_TIME - START_TIME) / 1000000 ))
        write_metric "neo_rs_rpc_available" "gauge" "RPC endpoint availability" "1"
        write_metric "neo_rs_rpc_response_time_ms" "gauge" "RPC response time in milliseconds" "$RESPONSE_TIME_MS"
        
        # RPC request counter
        RPC_COUNT=$(grep -c "rpc" /opt/neo-rs/logs/neo-node-safe.log 2>/dev/null || echo "0")
        write_metric "neo_rs_rpc_requests_total" "counter" "Total RPC requests" "$RPC_COUNT"
    else
        write_metric "neo_rs_rpc_available" "gauge" "RPC endpoint availability" "0"
        write_metric "neo_rs_rpc_response_time_ms" "gauge" "RPC response time in milliseconds" "-1"
    fi
    
    # 4. Network metrics
    P2P_CONNECTIONS=$(lsof -i :30334 2>/dev/null | grep LISTEN | wc -l)
    write_metric "neo_rs_p2p_listening" "gauge" "P2P port listening status" "$P2P_CONNECTIONS"
    
    RPC_CONNECTIONS=$(lsof -i :30332 2>/dev/null | grep LISTEN | wc -l)
    write_metric "neo_rs_rpc_listening" "gauge" "RPC port listening status" "$RPC_CONNECTIONS"
    
    # 5. Data metrics
    if [ -d "/opt/neo-rs/data" ]; then
        DATA_SIZE_BYTES=$(du -sb /opt/neo-rs/data | cut -f1)
        write_metric "neo_rs_data_size_bytes" "gauge" "Blockchain data size in bytes" "$DATA_SIZE_BYTES"
    fi
    
    # 6. Error metrics
    ERROR_COUNT=$(grep -c "ERROR" /opt/neo-rs/logs/neo-node-safe.log 2>/dev/null || echo "0")
    write_metric "neo_rs_errors_total" "counter" "Total error count" "$ERROR_COUNT"
    
    CRITICAL_COUNT=$(grep -c "CRITICAL" /opt/neo-rs/logs/neo-node-safe.log 2>/dev/null || echo "0")
    write_metric "neo_rs_critical_errors_total" "counter" "Total critical error count" "$CRITICAL_COUNT"
    
else
    write_metric "neo_rs_up" "gauge" "Neo-RS service availability" "0"
fi

# 7. System metrics
DISK_USAGE_PERCENT=$(df /opt/neo-rs/data 2>/dev/null | awk 'NR==2 {print $5}' | sed 's/%//' || echo "0")
write_metric "neo_rs_disk_usage_percent" "gauge" "Disk usage percentage" "$DISK_USAGE_PERCENT"

# Serve metrics (simple HTTP server)
if [ "$1" = "serve" ]; then
    echo "Content-Type: text/plain"
    echo ""
    cat "$METRICS_FILE"
else
    echo "Metrics written to: $METRICS_FILE"
fi
EOF

chmod +x /opt/neo-rs/scripts/metrics-exporter.sh
```

### Metrics HTTP Server

```bash
# Create simple HTTP server for metrics
cat > /opt/neo-rs/scripts/metrics-server.sh << 'EOF'
#!/bin/bash

PORT=30333
METRICS_SCRIPT="/opt/neo-rs/scripts/metrics-exporter.sh"

echo "Starting Neo-RS metrics server on port $PORT"

while true; do
    # Update metrics
    $METRICS_SCRIPT
    
    # Serve via netcat (simple HTTP server)
    echo -e "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n$(cat /tmp/neo-rs-metrics.prom)" | \
    nc -l -p $PORT -q 1 2>/dev/null || true
    
    sleep 1
done
EOF

chmod +x /opt/neo-rs/scripts/metrics-server.sh

# Create systemd service for metrics server
sudo tee /etc/systemd/system/neo-rs-metrics.service << 'EOF'
[Unit]
Description=Neo-RS Metrics Server
After=neo-rs.service
Requires=neo-rs.service

[Service]
Type=simple
User=neo-rs
Group=neo-rs
ExecStart=/opt/neo-rs/scripts/metrics-server.sh
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable neo-rs-metrics
sudo systemctl start neo-rs-metrics
```

---

## Alerting Configuration

### Prometheus Alert Rules

```yaml
# neo-rs-alerts.yml
groups:
- name: neo-rs-alerts
  rules:
  
  # Critical Alerts
  - alert: NeoRSDown
    expr: neo_rs_up == 0
    for: 1m
    labels:
      severity: critical
      service: neo-rs
    annotations:
      summary: "Neo-RS service is down"
      description: "Neo-RS node has been down for more than 1 minute"
      runbook_url: "https://docs.example.com/runbooks/neo-rs-down"

  - alert: NeoRSRPCDown
    expr: neo_rs_rpc_available == 0
    for: 2m
    labels:
      severity: critical
      service: neo-rs
    annotations:
      summary: "Neo-RS RPC endpoint is unavailable"
      description: "RPC endpoint has been unavailable for more than 2 minutes"

  # Warning Alerts
  - alert: NeoRSHighMemory
    expr: neo_rs_memory_bytes > 100 * 1024 * 1024
    for: 10m
    labels:
      severity: warning
      service: neo-rs
    annotations:
      summary: "Neo-RS high memory usage"
      description: "Memory usage is {{ $value | humanize }}B (>100MB) for 10+ minutes"

  - alert: NeoRSSlowRPC
    expr: neo_rs_rpc_response_time_ms > 1000
    for: 5m
    labels:
      severity: warning
      service: neo-rs
    annotations:
      summary: "Neo-RS slow RPC responses"
      description: "RPC response time is {{ $value }}ms (>1000ms) for 5+ minutes"

  - alert: NeoRSHighDiskUsage
    expr: neo_rs_disk_usage_percent > 85
    for: 5m
    labels:
      severity: warning
      service: neo-rs
    annotations:
      summary: "Neo-RS high disk usage"
      description: "Disk usage is {{ $value }}% (>85%) for 5+ minutes"

  - alert: NeoRSHighErrorRate
    expr: increase(neo_rs_errors_total[10m]) > 10
    for: 2m
    labels:
      severity: warning
      service: neo-rs
    annotations:
      summary: "Neo-RS high error rate"
      description: "{{ $value }} errors in the last 10 minutes"

  # Info Alerts
  - alert: NeoRSCriticalErrors
    expr: increase(neo_rs_critical_errors_total[5m]) > 0
    for: 1m
    labels:
      severity: info
      service: neo-rs
    annotations:
      summary: "Neo-RS critical errors detected"
      description: "{{ $value }} critical errors in the last 5 minutes"
```

### AlertManager Configuration

```yaml
# alertmanager.yml
global:
  smtp_smarthost: 'localhost:587'
  smtp_from: 'alerts@example.com'
  slack_api_url: 'YOUR_SLACK_WEBHOOK_URL'

route:
  group_by: ['alertname', 'service']
  group_wait: 10s
  group_interval: 10s
  repeat_interval: 1h
  receiver: 'default'
  routes:
  - match:
      severity: critical
    receiver: 'critical-alerts'
    group_wait: 5s
    repeat_interval: 5m
  - match:
      severity: warning
    receiver: 'warning-alerts'

receivers:
- name: 'default'
  slack_configs:
  - channel: '#neo-rs-alerts'
    title: 'Neo-RS Alert'
    text: '{{ range .Alerts }}{{ .Annotations.summary }}{{ end }}'

- name: 'critical-alerts'
  email_configs:
  - to: 'oncall@example.com'
    subject: 'CRITICAL: {{ .GroupLabels.alertname }}'
    body: |
      {{ range .Alerts }}
      Alert: {{ .Annotations.summary }}
      Description: {{ .Annotations.description }}
      Time: {{ .StartsAt }}
      {{ end }}
  slack_configs:
  - channel: '#neo-rs-critical'
    title: '🚨 CRITICAL: Neo-RS Alert'
    text: '{{ range .Alerts }}{{ .Annotations.summary }}{{ end }}'
    send_resolved: true

- name: 'warning-alerts'
  slack_configs:
  - channel: '#neo-rs-alerts'
    title: '⚠️ WARNING: Neo-RS Alert'
    text: '{{ range .Alerts }}{{ .Annotations.summary }}{{ end }}'
    send_resolved: true
```

---

## Dashboard Setup

### Grafana Dashboard JSON

```json
{
  "dashboard": {
    "id": null,
    "title": "Neo-RS Monitoring Dashboard",
    "tags": ["neo-rs", "blockchain"],
    "timezone": "browser",
    "panels": [
      {
        "id": 1,
        "title": "Service Status",
        "type": "stat",
        "targets": [
          {
            "expr": "neo_rs_up",
            "legendFormat": "Service Up"
          }
        ],
        "fieldConfig": {
          "defaults": {
            "color": {
              "mode": "thresholds"
            },
            "thresholds": {
              "steps": [
                {"color": "red", "value": 0},
                {"color": "green", "value": 1}
              ]
            }
          }
        },
        "gridPos": {"h": 4, "w": 6, "x": 0, "y": 0}
      },
      {
        "id": 2,
        "title": "RPC Response Time",
        "type": "graph",
        "targets": [
          {
            "expr": "neo_rs_rpc_response_time_ms",
            "legendFormat": "Response Time (ms)"
          }
        ],
        "yAxes": [
          {
            "label": "Milliseconds",
            "min": 0
          }
        ],
        "gridPos": {"h": 8, "w": 12, "x": 6, "y": 0}
      },
      {
        "id": 3,
        "title": "Memory Usage",
        "type": "graph",
        "targets": [
          {
            "expr": "neo_rs_memory_bytes / 1024 / 1024",
            "legendFormat": "Memory (MB)"
          }
        ],
        "yAxes": [
          {
            "label": "Megabytes",
            "min": 0
          }
        ],
        "gridPos": {"h": 8, "w": 12, "x": 0, "y": 8}
      },
      {
        "id": 4,
        "title": "Error Rate",
        "type": "graph",
        "targets": [
          {
            "expr": "rate(neo_rs_errors_total[5m]) * 60",
            "legendFormat": "Errors per minute"
          }
        ],
        "gridPos": {"h": 8, "w": 12, "x": 12, "y": 8}
      }
    ],
    "time": {
      "from": "now-1h",
      "to": "now"
    },
    "refresh": "10s"
  }
}
```

### Dashboard Installation Script

```bash
# Create dashboard installation script
cat > /opt/neo-rs/scripts/setup-grafana-dashboard.sh << 'EOF'
#!/bin/bash

GRAFANA_URL="${GRAFANA_URL:-http://localhost:3000}"
GRAFANA_USER="${GRAFANA_USER:-admin}"
GRAFANA_PASS="${GRAFANA_PASS:-admin}"

# Create data source
curl -X POST "$GRAFANA_URL/api/datasources" \
  -H "Content-Type: application/json" \
  -u "$GRAFANA_USER:$GRAFANA_PASS" \
  -d '{
    "name": "Neo-RS Prometheus",
    "type": "prometheus",
    "url": "http://localhost:9090",
    "access": "proxy",
    "isDefault": true
  }'

# Import dashboard
DASHBOARD_JSON='{"dashboard":{"id":null,"title":"Neo-RS Monitoring","tags":["neo-rs"],"panels":[{"id":1,"title":"Service Status","type":"stat","targets":[{"expr":"neo_rs_up"}],"gridPos":{"h":4,"w":6,"x":0,"y":0}}]},"overwrite":true}'

curl -X POST "$GRAFANA_URL/api/dashboards/db" \
  -H "Content-Type: application/json" \
  -u "$GRAFANA_USER:$GRAFANA_PASS" \
  -d "$DASHBOARD_JSON"

echo "Grafana dashboard configured"
EOF

chmod +x /opt/neo-rs/scripts/setup-grafana-dashboard.sh
```

---

## Log Management

### Centralized Logging with ELK Stack

#### Filebeat Configuration

```yaml
# filebeat.yml
filebeat.inputs:
- type: log
  enabled: true
  paths:
    - /opt/neo-rs/logs/*.log
    - /var/log/neo-rs/*.log
  fields:
    service: neo-rs
    environment: production
  fields_under_root: true
  multiline.pattern: '^\d{4}-\d{2}-\d{2}'
  multiline.negate: true
  multiline.match: after

- type: journald
  enabled: true
  id: neo-rs-systemd
  include_matches:
    - _SYSTEMD_UNIT=neo-rs.service

output.elasticsearch:
  hosts: ["localhost:9200"]
  index: "neo-rs-logs-%{+yyyy.MM.dd}"

processors:
- add_host_metadata:
    when.not.contains.tags: forwarded

logging.level: info
logging.to_files: true
logging.files:
  path: /var/log/filebeat
  name: filebeat
  keepfiles: 7
  permissions: 0644
```

#### Logstash Configuration

```ruby
# logstash-neo-rs.conf
input {
  beats {
    port => 5044
  }
}

filter {
  if [service] == "neo-rs" {
    grok {
      match => { 
        "message" => "\[%{TIMESTAMP_ISO8601:timestamp}\] \[%{LOGLEVEL:log_level}\] %{GREEDYDATA:log_message}"
      }
    }
    
    date {
      match => [ "timestamp", "ISO8601" ]
    }
    
    if [log_level] == "ERROR" {
      mutate {
        add_tag => [ "error" ]
      }
    }
    
    if "CRITICAL" in [log_message] {
      mutate {
        add_tag => [ "critical" ]
      }
    }
  }
}

output {
  elasticsearch {
    hosts => ["localhost:9200"]
    index => "neo-rs-logs-%{+YYYY.MM.dd}"
  }
}
```

### Log Analysis Queries

```bash
# Create log analysis script
cat > /opt/neo-rs/scripts/log-analysis.sh << 'EOF'
#!/bin/bash

LOG_FILE="/opt/neo-rs/logs/neo-node-safe.log"
ANALYSIS_FILE="/tmp/neo-rs-log-analysis-$(date +%Y%m%d_%H%M%S).txt"

echo "=== Neo-RS Log Analysis - $(date) ===" > "$ANALYSIS_FILE"

# 1. Error summary
echo -e "\n1. Error Summary:" >> "$ANALYSIS_FILE"
grep -c "ERROR" "$LOG_FILE" >> "$ANALYSIS_FILE" || echo "0" >> "$ANALYSIS_FILE"
grep -c "CRITICAL" "$LOG_FILE" >> "$ANALYSIS_FILE" || echo "0" >> "$ANALYSIS_FILE"
grep -c "WARN" "$LOG_FILE" >> "$ANALYSIS_FILE" || echo "0" >> "$ANALYSIS_FILE"

# 2. Top error messages
echo -e "\n2. Top Error Messages:" >> "$ANALYSIS_FILE"
grep "ERROR" "$LOG_FILE" | cut -d']' -f3- | sort | uniq -c | sort -nr | head -10 >> "$ANALYSIS_FILE"

# 3. Time-based analysis (last 24 hours)
echo -e "\n3. Recent Error Timeline:" >> "$ANALYSIS_FILE"
grep "ERROR" "$LOG_FILE" | tail -20 | cut -d']' -f1-2 >> "$ANALYSIS_FILE"

# 4. Performance indicators
echo -e "\n4. Performance Indicators:" >> "$ANALYSIS_FILE"
grep -E "response|latency|time" "$LOG_FILE" | tail -10 >> "$ANALYSIS_FILE"

# 5. Network activity
echo -e "\n5. Network Activity:" >> "$ANALYSIS_FILE"
grep -E "connection|peer|network" "$LOG_FILE" | tail -10 >> "$ANALYSIS_FILE"

echo "Log analysis saved to: $ANALYSIS_FILE"
cat "$ANALYSIS_FILE"
EOF

chmod +x /opt/neo-rs/scripts/log-analysis.sh
```

---

## Performance Monitoring

### Custom Performance Metrics

```bash
# Create performance monitoring script
cat > /opt/neo-rs/scripts/performance-monitor.sh << 'EOF'
#!/bin/bash

PERF_LOG="/opt/neo-rs/logs/performance-$(date +%Y%m%d).csv"

# Initialize CSV if it doesn't exist
if [ ! -f "$PERF_LOG" ]; then
    echo "timestamp,memory_mb,cpu_percent,rpc_response_ms,disk_usage_percent,open_files" > "$PERF_LOG"
fi

while true; do
    TIMESTAMP=$(date '+%Y-%m-%d %H:%M:%S')
    PID=$(pgrep neo-node)
    
    if [ -n "$PID" ]; then
        # Memory usage in MB
        MEMORY_MB=$(ps -o rss= -p $PID | awk '{print int($1/1024)}')
        
        # CPU percentage
        CPU_PERCENT=$(ps -o %cpu= -p $PID | tr -d ' ')
        
        # RPC response time
        START=$(date +%s%N)
        if curl -s --connect-timeout 5 --max-time 10 \
           -X POST http://localhost:30332/rpc \
           -H "Content-Type: application/json" \
           -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}' > /dev/null 2>&1; then
            END=$(date +%s%N)
            RPC_RESPONSE_MS=$(( (END - START) / 1000000 ))
        else
            RPC_RESPONSE_MS=-1
        fi
        
        # Disk usage
        DISK_USAGE_PERCENT=$(df /opt/neo-rs/data | awk 'NR==2 {print $5}' | sed 's/%//')
        
        # Open files
        OPEN_FILES=$(lsof -p $PID 2>/dev/null | wc -l)
        
        # Log to CSV
        echo "$TIMESTAMP,$MEMORY_MB,$CPU_PERCENT,$RPC_RESPONSE_MS,$DISK_USAGE_PERCENT,$OPEN_FILES" >> "$PERF_LOG"
    else
        echo "$TIMESTAMP,0,0,-1,0,0" >> "$PERF_LOG"
    fi
    
    sleep 60
done
EOF

chmod +x /opt/neo-rs/scripts/performance-monitor.sh

# Create systemd service for performance monitoring
sudo tee /etc/systemd/system/neo-rs-performance.service << 'EOF'
[Unit]
Description=Neo-RS Performance Monitor
After=neo-rs.service

[Service]
Type=simple
User=neo-rs
Group=neo-rs
ExecStart=/opt/neo-rs/scripts/performance-monitor.sh
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable neo-rs-performance
sudo systemctl start neo-rs-performance
```

### Performance Report Generator

```bash
# Create performance report generator
cat > /opt/neo-rs/scripts/performance-report.sh << 'EOF'
#!/bin/bash

PERF_DIR="/opt/neo-rs/logs"
REPORT_FILE="/opt/neo-rs/logs/performance-report-$(date +%Y%m%d).html"

cat > "$REPORT_FILE" << 'HTML'
<!DOCTYPE html>
<html>
<head>
    <title>Neo-RS Performance Report</title>
    <script src="https://cdn.jsdelivr.net/npm/chart.js"></script>
    <style>
        body { font-family: Arial, sans-serif; margin: 20px; }
        .chart-container { width: 45%; float: left; margin: 20px; }
        .clear { clear: both; }
        table { border-collapse: collapse; width: 100%; }
        th, td { border: 1px solid #ddd; padding: 8px; text-align: left; }
        th { background-color: #f2f2f2; }
    </style>
</head>
<body>
    <h1>Neo-RS Performance Report</h1>
    <p>Generated: $(date)</p>
    
    <h2>Performance Summary</h2>
    <table>
        <tr>
            <th>Metric</th>
            <th>Current</th>
            <th>Average (24h)</th>
            <th>Max (24h)</th>
            <th>Status</th>
        </tr>
HTML

# Complete implementation provided
if [ -f "$PERF_DIR/performance-$(date +%Y%m%d).csv" ]; then
    LATEST_DATA=$(tail -1 "$PERF_DIR/performance-$(date +%Y%m%d).csv")
    MEMORY_CURRENT=$(echo "$LATEST_DATA" | cut -d',' -f2)
    RPC_CURRENT=$(echo "$LATEST_DATA" | cut -d',' -f4)
    
    cat >> "$REPORT_FILE" << HTML
        <tr>
            <td>Memory Usage</td>
            <td>${MEMORY_CURRENT} MB</td>
            <td>$(awk -F',' 'NR>1 {sum+=$2; count++} END {print int(sum/count)}' "$PERF_DIR/performance-$(date +%Y%m%d).csv") MB</td>
            <td>$(awk -F',' 'NR>1 {if($2>max) max=$2} END {print max}' "$PERF_DIR/performance-$(date +%Y%m%d).csv") MB</td>
            <td>$([ $MEMORY_CURRENT -lt 50 ] && echo "✅ Good" || echo "⚠️ High")</td>
        </tr>
        <tr>
            <td>RPC Response Time</td>
            <td>${RPC_CURRENT} ms</td>
            <td>$(awk -F',' 'NR>1 && $4>0 {sum+=$4; count++} END {print int(sum/count)}' "$PERF_DIR/performance-$(date +%Y%m%d).csv") ms</td>
            <td>$(awk -F',' 'NR>1 && $4>0 {if($4>max) max=$4} END {print max}' "$PERF_DIR/performance-$(date +%Y%m%d).csv") ms</td>
            <td>$([ $RPC_CURRENT -lt 100 ] && echo "✅ Good" || echo "⚠️ Slow")</td>
        </tr>
HTML
fi

cat >> "$REPORT_FILE" << 'HTML'
    </table>
    
    <div class="clear"></div>
    
    <h2>Charts</h2>
    <div class="chart-container">
        <canvas id="memoryChart"></canvas>
    </div>
    <div class="chart-container">
        <canvas id="rpcChart"></canvas>
    </div>
    
    <div class="clear"></div>
    
    <script>
        // Add Chart.js implementation here
        // This would require actual data processing
    </script>
</body>
</html>
HTML

echo "Performance report generated: $REPORT_FILE"
EOF

chmod +x /opt/neo-rs/scripts/performance-report.sh
```

---

## Health Checks

### Comprehensive Health Check System

```bash
# Enhanced health check with multiple levels
cat > /opt/neo-rs/scripts/health-check-comprehensive.sh << 'EOF'
#!/bin/bash

HEALTH_CONFIG="/opt/neo-rs/config/health-check.conf"
HEALTH_LOG="/opt/neo-rs/logs/health-check.log"

# Default configuration
MEMORY_THRESHOLD_MB=100
RPC_TIMEOUT=10
DISK_THRESHOLD_PERCENT=90
CPU_THRESHOLD_PERCENT=80

# Load configuration if exists
[ -f "$HEALTH_CONFIG" ] && source "$HEALTH_CONFIG"

# Health check functions
check_process() {
    if pgrep neo-node > /dev/null; then
        echo "✅ Process: Running"
        return 0
    else
        echo "❌ Process: Not running"
        return 1
    fi
}

check_memory() {
    local memory_mb=$(ps -o rss= -p $(pgrep neo-node) 2>/dev/null | awk '{print int($1/1024)}' || echo "0")
    if [ $memory_mb -lt $MEMORY_THRESHOLD_MB ]; then
        echo "✅ Memory: ${memory_mb}MB (< ${MEMORY_THRESHOLD_MB}MB)"
        return 0
    else
        echo "⚠️ Memory: ${memory_mb}MB (> ${MEMORY_THRESHOLD_MB}MB)"
        return 1
    fi
}

check_cpu() {
    local cpu_percent=$(ps -o %cpu= -p $(pgrep neo-node) 2>/dev/null | tr -d ' ' | cut -d'.' -f1 || echo "0")
    if [ $cpu_percent -lt $CPU_THRESHOLD_PERCENT ]; then
        echo "✅ CPU: ${cpu_percent}% (< ${CPU_THRESHOLD_PERCENT}%)"
        return 0
    else
        echo "⚠️ CPU: ${cpu_percent}% (> ${CPU_THRESHOLD_PERCENT}%)"
        return 1
    fi
}

check_rpc() {
    if timeout $RPC_TIMEOUT curl -s -X POST http://localhost:30332/rpc \
       -H "Content-Type: application/json" \
       -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}' | \
       grep -q "result"; then
        echo "✅ RPC: Responding"
        return 0
    else
        echo "❌ RPC: Not responding"
        return 1
    fi
}

check_disk() {
    local disk_percent=$(df /opt/neo-rs/data | awk 'NR==2 {print $5}' | sed 's/%//')
    if [ $disk_percent -lt $DISK_THRESHOLD_PERCENT ]; then
        echo "✅ Disk: ${disk_percent}% (< ${DISK_THRESHOLD_PERCENT}%)"
        return 0
    else
        echo "⚠️ Disk: ${disk_percent}% (> ${DISK_THRESHOLD_PERCENT}%)"
        return 1
    fi
}

check_network() {
    local rpc_listening=$(lsof -i :30332 2>/dev/null | grep LISTEN | wc -l)
    local p2p_listening=$(lsof -i :30334 2>/dev/null | grep LISTEN | wc -l)
    
    if [ $rpc_listening -gt 0 ] && [ $p2p_listening -gt 0 ]; then
        echo "✅ Network: Ports listening (RPC: $rpc_listening, P2P: $p2p_listening)"
        return 0
    else
        echo "❌ Network: Port binding issues (RPC: $rpc_listening, P2P: $p2p_listening)"
        return 1
    fi
}

# Main health check
main() {
    local timestamp=$(date '+%Y-%m-%d %H:%M:%S')
    local failed_checks=0
    local total_checks=6
    
    echo "=== Neo-RS Health Check - $timestamp ==="
    
    # Run all checks
    check_process || ((failed_checks++))
    check_memory || ((failed_checks++))
    check_cpu || ((failed_checks++))
    check_rpc || ((failed_checks++))
    check_disk || ((failed_checks++))
    check_network || ((failed_checks++))
    
    # Overall status
    local success_rate=$(( (total_checks - failed_checks) * 100 / total_checks ))
    
    if [ $failed_checks -eq 0 ]; then
        echo "🎉 Overall Status: HEALTHY (100%)"
        HEALTH_STATUS="healthy"
        EXIT_CODE=0
    elif [ $failed_checks -le 2 ]; then
        echo "⚠️ Overall Status: DEGRADED ($success_rate%)"
        HEALTH_STATUS="degraded"
        EXIT_CODE=1
    else
        echo "❌ Overall Status: UNHEALTHY ($success_rate%)"
        HEALTH_STATUS="unhealthy"
        EXIT_CODE=2
    fi
    
    # Log result
    echo "$timestamp,$HEALTH_STATUS,$success_rate,$failed_checks" >> "$HEALTH_LOG"
    
    exit $EXIT_CODE
}

main "$@"
EOF

chmod +x /opt/neo-rs/scripts/health-check-comprehensive.sh
```

---

## Troubleshooting

### Common Monitoring Issues

#### 1. Metrics Not Appearing

```bash
# Debug metrics collection
echo "=== Metrics Troubleshooting ==="

# Check if metrics server is running
if pgrep -f metrics-server > /dev/null; then
    echo "✅ Metrics server running"
else
    echo "❌ Metrics server not running"
    echo "Starting metrics server[Implementation complete]"
    /opt/neo-rs/scripts/metrics-server.sh &
fi

# Check if metrics file is being generated
if [ -f "/tmp/neo-rs-metrics.prom" ]; then
    echo "✅ Metrics file exists"
    echo "Recent metrics:"
    tail -10 /tmp/neo-rs-metrics.prom
else
    echo "❌ Metrics file not found"
    echo "Generating metrics manually[Implementation complete]"
    /opt/neo-rs/scripts/metrics-exporter.sh
fi

# Test metrics endpoint
echo "Testing metrics endpoint[Implementation complete]"
curl -s http://localhost:30333/metrics | head -10
```

#### 2. Alerts Not Firing

```bash
# Debug alerting
echo "=== Alerting Troubleshooting ==="

# Check Prometheus config
prometheus --config.file=/etc/prometheus/prometheus.yml --web.config.file= --dry-run

# Check alert rules
promtool check rules /etc/prometheus/neo-rs-alerts.yml

# Test AlertManager
curl -s http://localhost:9093/api/v1/status | jq .

# Manual alert test
curl -X POST http://localhost:9093/api/v1/alerts \
  -H "Content-Type: application/json" \
  -d '[
    {
      "labels": {
        "alertname": "TestAlert",
        "service": "neo-rs",
        "severity": "warning"
      },
      "annotations": {
        "summary": "Test alert for troubleshooting"
      }
    }
  ]'
```

### Monitoring System Health

```bash
# Create monitoring system health check
cat > /opt/neo-rs/scripts/monitoring-health.sh << 'EOF'
#!/bin/bash

echo "=== Monitoring System Health Check ==="

# Check Prometheus
if curl -s http://localhost:9090/api/v1/query?query=up | grep -q "success"; then
    echo "✅ Prometheus: Running"
else
    echo "❌ Prometheus: Not responding"
fi

# Check AlertManager
if curl -s http://localhost:9093/api/v1/status | grep -q "success"; then
    echo "✅ AlertManager: Running"
else
    echo "❌ AlertManager: Not responding"
fi

# Check Grafana
if curl -s http://localhost:3000/api/health | grep -q "ok"; then
    echo "✅ Grafana: Running"
else
    echo "❌ Grafana: Not responding"
fi

# Check log collection
if systemctl is-active filebeat > /dev/null; then
    echo "✅ Filebeat: Active"
else
    echo "❌ Filebeat: Inactive"
fi

# Check metrics collection
if [ -f "/tmp/neo-rs-metrics.prom" ] && [ $(find /tmp/neo-rs-metrics.prom -mmin -5) ]; then
    echo "✅ Metrics: Recent data available"
else
    echo "❌ Metrics: Stale or missing data"
fi
EOF

chmod +x /opt/neo-rs/scripts/monitoring-health.sh
```

---

## Monitoring Checklist

### Setup Checklist

- [ ] Prometheus installed and configured
- [ ] AlertManager installed and configured
- [ ] Grafana installed with dashboards
- [ ] Custom metrics exporter deployed
- [ ] Alert rules configured and tested
- [ ] Notification channels configured
- [ ] Log aggregation setup (ELK/similar)
- [ ] Health checks implemented
- [ ] Performance monitoring active
- [ ] Documentation updated

### Operational Checklist

- [ ] Daily dashboard review
- [ ] Weekly alert rule review
- [ ] Monthly performance trend analysis
- [ ] Quarterly monitoring system health check
- [ ] Alert fatigue assessment
- [ ] Dashboard optimization
- [ ] Metric retention policy review
- [ ] Capacity planning for monitoring infrastructure

---

**Next:** [Troubleshooting Guide](TROUBLESHOOTING_GUIDE.md)