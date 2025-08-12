# Neo Rust Node Monitoring Guide

## Overview

The Neo Rust node provides comprehensive metrics collection and monitoring capabilities through a Prometheus-compatible metrics endpoint. This guide explains how to set up monitoring, interpret metrics, and create alerts.

## 🚀 Quick Start

### Enable Metrics

Add to your node configuration:

```toml
[monitoring]
prometheus_enabled = true
prometheus_port = 9090
```

### Access Metrics

Once enabled, metrics are available at:
```
http://localhost:9090/metrics
```

## 📊 Available Metrics

### Blockchain Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `neo_block_height` | Gauge | Current blockchain height |
| `neo_block_processing_time_seconds` | Histogram | Time to process each block |
| `neo_blocks_processed_total` | Counter | Total number of blocks processed |

### Transaction Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `neo_tx_pool_size` | Gauge | Number of transactions in mempool |
| `neo_tx_processed_total` | Counter | Total transactions processed by type |
| `neo_tx_validation_time_seconds` | Histogram | Transaction validation time |

### Network Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `neo_peer_count` | Gauge | Number of connected peers by state |
| `neo_messages_received_total` | Counter | Messages received by type |
| `neo_messages_sent_total` | Counter | Messages sent by type |
| `neo_bytes_received_total` | Counter | Total bytes received |
| `neo_bytes_sent_total` | Counter | Total bytes sent |

### Consensus Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `neo_consensus_view` | Gauge | Current consensus view number |
| `neo_consensus_rounds_total` | Counter | Consensus rounds by result |
| `neo_consensus_duration_seconds` | Histogram | Time to reach consensus |

### VM Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `neo_vm_executions_total` | Counter | VM executions by result |
| `neo_vm_gas_consumed_total` | Counter | Total GAS consumed |
| `neo_vm_execution_time_seconds` | Histogram | VM execution time |

### Storage Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `neo_storage_size_bytes` | Gauge | Storage size by type |
| `neo_storage_operations_total` | Counter | Storage operations by type and result |

### RPC Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `neo_rpc_requests_total` | Counter | RPC requests by method and status |
| `neo_rpc_request_duration_seconds` | Histogram | RPC request duration by method |

### System Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `neo_uptime_seconds` | Gauge | Node uptime |
| `neo_memory_usage_bytes` | Gauge | Memory usage |
| `neo_cpu_usage_percent` | Gauge | CPU usage percentage |

## 🔧 Prometheus Setup

### 1. Install Prometheus

```bash
# Download Prometheus
wget https://github.com/prometheus/prometheus/releases/download/v2.45.0/prometheus-2.45.0.linux-amd64.tar.gz
tar xvf prometheus-2.45.0.linux-amd64.tar.gz
cd prometheus-2.45.0.linux-amd64
```

### 2. Configure Prometheus

Create `prometheus.yml`:

```yaml
global:
  scrape_interval: 15s
  evaluation_interval: 15s

scrape_configs:
  - job_name: 'neo-node'
    static_configs:
      - targets: ['localhost:9090']
        labels:
          network: 'testnet'
          node_type: 'full'
```

### 3. Start Prometheus

```bash
./prometheus --config.file=prometheus.yml
```

Access Prometheus UI at `http://localhost:9090`

## 📈 Grafana Dashboard

### 1. Install Grafana

```bash
# Add Grafana repository
sudo apt-get install -y software-properties-common
sudo add-apt-repository "deb https://packages.grafana.com/oss/deb stable main"
wget -q -O - https://packages.grafana.com/gpg.key | sudo apt-key add -

# Install
sudo apt-get update
sudo apt-get install grafana

# Start service
sudo systemctl start grafana-server
sudo systemctl enable grafana-server
```

### 2. Import Dashboard

1. Access Grafana at `http://localhost:3000` (default: admin/admin)
2. Add Prometheus data source
3. Import the Neo monitoring dashboard (JSON provided below)

### 3. Neo Dashboard JSON

Save as `neo-dashboard.json`:

```json
{
  "dashboard": {
    "title": "Neo Node Monitoring",
    "panels": [
      {
        "title": "Block Height",
        "targets": [
          {
            "expr": "neo_block_height",
            "legendFormat": "Height"
          }
        ],
        "gridPos": {"x": 0, "y": 0, "w": 6, "h": 8}
      },
      {
        "title": "Peer Count",
        "targets": [
          {
            "expr": "neo_peer_count",
            "legendFormat": "{{state}}"
          }
        ],
        "gridPos": {"x": 6, "y": 0, "w": 6, "h": 8}
      },
      {
        "title": "Transaction Pool Size",
        "targets": [
          {
            "expr": "neo_tx_pool_size",
            "legendFormat": "Pool Size"
          }
        ],
        "gridPos": {"x": 12, "y": 0, "w": 6, "h": 8}
      },
      {
        "title": "Network Traffic",
        "targets": [
          {
            "expr": "rate(neo_bytes_received_total[5m])",
            "legendFormat": "Received"
          },
          {
            "expr": "rate(neo_bytes_sent_total[5m])",
            "legendFormat": "Sent"
          }
        ],
        "gridPos": {"x": 18, "y": 0, "w": 6, "h": 8}
      }
    ]
  }
}
```

## 🚨 Alert Rules

### Prometheus Alert Configuration

Create `alerts.yml`:

```yaml
groups:
  - name: neo_alerts
    interval: 30s
    rules:
      # Node is not syncing
      - alert: NodeNotSyncing
        expr: rate(neo_block_height[5m]) == 0
        for: 10m
        labels:
          severity: warning
        annotations:
          summary: "Neo node is not syncing"
          description: "Block height has not increased in 10 minutes"
      
      # Low peer count
      - alert: LowPeerCount
        expr: neo_peer_count{state="connected"} < 3
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Low peer count"
          description: "Connected peers: {{ $value }}"
      
      # High memory usage
      - alert: HighMemoryUsage
        expr: neo_memory_usage_bytes > 8589934592  # 8GB
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "High memory usage"
          description: "Memory usage: {{ $value | humanize }}B"
      
      # Transaction pool overflow
      - alert: TxPoolOverflow
        expr: neo_tx_pool_size > 5000
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Transaction pool overflow"
          description: "Pool size: {{ $value }}"
      
      # Consensus failures
      - alert: ConsensusFailures
        expr: rate(neo_consensus_rounds_total{result="failure"}[5m]) > 0.1
        for: 5m
        labels:
          severity: critical
        annotations:
          summary: "High consensus failure rate"
          description: "Failure rate: {{ $value }}"
```

## 📋 Monitoring Best Practices

### 1. Key Metrics to Watch

**Health Indicators:**
- Block height progression
- Peer count stability
- Transaction validation times
- Consensus success rate

**Performance Indicators:**
- Block processing time
- VM execution time
- RPC response times
- Network throughput

**Resource Indicators:**
- Memory usage
- CPU usage
- Storage growth
- File descriptor usage

### 2. Baseline Establishment

Monitor for 24-48 hours to establish normal baselines:

```bash
# Record baseline metrics
curl -s http://localhost:9090/metrics | grep -E "block_height|peer_count|memory_usage" > baseline.txt
```

### 3. Performance Tuning

Based on metrics, tune configuration:

```toml
# If high block processing time
[vm]
max_stack_size = 2048
max_invocation_stack_size = 1024

# If high memory usage
[storage]
cache_size = "1GB"  # Reduce cache

# If network congestion
[network]
max_peers = 20  # Increase peers
```

## 🔍 Troubleshooting with Metrics

### Node Not Syncing

```promql
# Check block height progression
neo_block_height

# Check peer connections
neo_peer_count{state="connected"}

# Check network messages
rate(neo_messages_received_total[5m])
```

### High Resource Usage

```promql
# Memory usage over time
neo_memory_usage_bytes

# Storage operations
rate(neo_storage_operations_total[5m])

# VM gas consumption
rate(neo_vm_gas_consumed_total[5m])
```

### Network Issues

```promql
# Message errors
neo_messages_received_total{type="error"}

# Bandwidth usage
rate(neo_bytes_received_total[5m])
rate(neo_bytes_sent_total[5m])
```

## 📊 Custom Queries

### Sync Progress

```promql
# Blocks per minute
rate(neo_blocks_processed_total[1m]) * 60

# Estimated sync time (assuming known target height)
(1000000 - neo_block_height) / (rate(neo_blocks_processed_total[5m]) * 60)
```

### Transaction Throughput

```promql
# Transactions per second
rate(neo_tx_processed_total[1m])

# Average validation time
rate(neo_tx_validation_time_seconds_sum[5m]) / rate(neo_tx_validation_time_seconds_count[5m])
```

### Consensus Performance

```promql
# Success rate
rate(neo_consensus_rounds_total{result="success"}[5m]) / rate(neo_consensus_rounds_total[5m])

# Average consensus time
rate(neo_consensus_duration_seconds_sum[5m]) / rate(neo_consensus_duration_seconds_count[5m])
```

## 🛠️ Advanced Monitoring

### Log Aggregation

Integrate with ELK stack:

```bash
# Install Filebeat
curl -L -O https://artifacts.elastic.co/downloads/beats/filebeat/filebeat-8.8.0-linux-x86_64.tar.gz
tar xzvf filebeat-8.8.0-linux-x86_64.tar.gz

# Configure for Neo logs
cat > filebeat.yml <<EOF
filebeat.inputs:
- type: log
  paths:
    - /opt/neo-rs/testnet-node.log
  json.keys_under_root: true

output.elasticsearch:
  hosts: ["localhost:9200"]
EOF
```

### Distributed Tracing

For complex debugging, integrate OpenTelemetry:

```rust
// In your node code
use opentelemetry::{global, sdk::trace as sdktrace};

fn init_tracing() {
    let tracer = sdktrace::TracerProvider::builder()
        .with_simple_exporter(opentelemetry_jaeger::new_pipeline())
        .build();
    global::set_tracer_provider(tracer);
}
```

## 📱 Mobile Alerts

### PagerDuty Integration

```yaml
# In alertmanager.yml
route:
  receiver: 'pagerduty'
  
receivers:
  - name: 'pagerduty'
    pagerduty_configs:
      - service_key: 'YOUR_SERVICE_KEY'
```

### Telegram Alerts

Use prometheus-telegram-alert:

```bash
docker run -d \
  -e TELEGRAM_TOKEN="YOUR_BOT_TOKEN" \
  -e TELEGRAM_CHAT_ID="YOUR_CHAT_ID" \
  -p 9087:9087 \
  metalmatze/alertmanager-telegram
```

## 📚 References

- [Prometheus Documentation](https://prometheus.io/docs/)
- [Grafana Documentation](https://grafana.com/docs/)
- [Neo Documentation](https://docs.neo.org/)

---

For additional help with monitoring setup, please refer to the Neo community resources or open an issue in the repository.