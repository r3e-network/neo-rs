# Neo-N3 Production Node Monitoring & Alerting Guide

## Overview

This document provides complete documentation for the production-grade monitoring and alerting system deployed with the optimized Neo-N3 testnet node. The monitoring stack includes:

- **Prometheus**: Metrics collection and storage
- **Alertmanager**: Alert routing and notification
- **Grafana**: Visualization and dashboards
- **Health endpoints**: Liveness and readiness probes

## Quick Start

### 1. Verify Prometheus Scrape Endpoint

```bash
curl http://127.0.0.1:9090/metrics | head -n 50
```

Expected output should include:
- `neo_sync_current_height`
- `neo_peer_count`
- `neo_cuckoo_hash_lookups_total`
- `neo_simd_blake2b_throughput_bytes_per_second`

### 2. Check Health Endpoints

**Liveness Probe:**
```bash
curl http://127.0.0.1:9090/healthz
```

**Readiness Probe:**
```bash
curl http://127.0.0.1:9090/ready
```

Both should return HTTP 200 with JSON status:
```json
{
  "status": "ok",
  "version": "0.17.0",
  "block_height": 1234567,
  "header_height": 1234580,
  "peer_count": 25,
  "cuckoo_hash": "active",
  "batch_verification": "active",
  "simd_blake2b": "active",
  "prefetch_pipeline": "active"
}
```

### 3. Import Grafana Dashboard

```bash
# Using Grafana CLI (if available)
grafana-cli plugins install grafana-piechart-panel

# Or import via UI:
# 1. Navigate to Dashboards > Import
# 2. Upload config/grafana/neo-node-dashboard.json
# 3. Select Prometheus data source
# 4. Click Import
```

## Metrics Reference

### Blockchain Metrics

| Metric Name | Type | Description |
|------------|------|-------------|
| `neo_block_height` | Gauge | Current synchronized block height |
| `neo_header_height` | Gauge | Highest header seen from peers |
| `neo_header_lag` | Gauge | Difference between header and block height |
| `neo_sync_current_height` | Gauge | Current syncing block height |
| `neo_sync_tip_height` | Gauge | Network tip height |
| `neo_blocks_processed_total` | Counter | Total blocks processed since start |
| `neo_transactions_verified_total` | Counter | Total transactions verified |

### Network Metrics

| Metric Name | Type | Description |
|------------|------|-------------|
| `neo_peer_count` | Gauge | Number of connected P2P peers |
| `neo_p2p_timeouts_handshake` | Gauge | Handshake timeout count |
| `neo_p2p_timeouts_read` | Gauge | Read timeout count |
| `neo_p2p_timeouts_write` | Gauge | Write timeout count |

### Mempool Metrics

| Metric Name | Type | Description |
|------------|------|-------------|
| `neo_mempool_size` | Gauge | Number of transactions in mempool |

### State Root Metrics

| Metric Name | Type | Description |
|------------|------|-------------|
| `neo_state_local_root_index` | Gauge | Local state root index |
| `neo_state_validated_root_index` | Gauge | Validated state root index |
| `neo_state_validated_lag` | Gauge | Lag between local and validated roots |
| `neo_state_roots_accepted_total` | Gauge | Accepted state roots count |
| `neo_state_roots_rejected_total` | Gauge | Rejected state roots count |

### Phase 1 Optimization Metrics

#### Cuckoo Hash Lookup Table

```promql
# Successful lookups
neo_cuckoo_hash_lookups_total

# Failed lookups (misses)
neo_cuckoo_hash_lookups_miss_total

# Hit rate calculation
(sum(rate(neo_cuckoo_hash_lookups_total[1h])) 
 / (sum(rate(neo_cuckoo_hash_lookups_total[1h])) 
 + sum(rate(neo_cuckoo_hash_lookups_miss_total[1h]))) * 100)
```

**Target:** Hit rate > 70%

**Syscall Lookup Latency Histogram:**
```promql
# P50 latency
histogram_quantile(0.50, rate(get_syscall_lookup_latency_bucket[5m]))

# P99 latency
histogram_quantile(0.99, rate(get_syscall_lookup_latency_bucket[5m]))

# Alert threshold: > 500ns average
```

#### Batch Signature Verification

```promql
# Total signatures verified
neo_batch_signatures_verified_total

# Average batch efficiency
neo_average_batch_size

# Target: >= 256 signatures per batch
```

#### SIMD BLAKE2b Hashing

```promql
# Throughput in bytes/second
neo_simd_blake2b_throughput_bytes_per_second

# Convert to MB/s for visualization
neo_simd_blake2b_throughput_bytes_per_second / 1000000

# Expected: > 500 MB/s
```

### Prefetch Pipeline Metrics

```promql
# Latency by stage (in seconds)
rate(prefetch_stage_latencies_bucket{stage="io"}[5m])
rate(prefetch_stage_latencies_bucket{stage="verify"}[5m])
rate(prefetch_stage_latencies_bucket{stage="execute"}[5m])

# Check if pipeline is stalled
rate(prefetch_stage_latencies_bucket{stage="execute"}[5m]) == 0
```

### System Resource Metrics

```promql
# CPU usage (0.0 to 1.0 scale)
cpu_usage_ratio

# Memory usage in bytes
memory_usage_bytes

# Total system memory
system_memory_total_bytes

# Memory percentage
(memory_usage_bytes / system_memory_total_bytes) * 100
```

### Fast Sync Mode Flag

```promql
# 1 = fast sync enabled, 0 = disabled
neo_fast_sync_enabled
```

### Block Processing Duration

```promql
# Block processing time histogram
block_processing_duration_bucket

# P99 block processing time
histogram_quantile(0.99, rate(block_processing_duration_bucket[5m]))

# Alert threshold: > 2 seconds
```

## Alert Rules

### Critical Alerts

#### 1. TestnetNodeSyncLag
**Trigger:** Node falls >1000 blocks behind tip  
**Severity:** CRITICAL  
**Expression:**
```promql
neo_sync_tip_height - neo_sync_current_height > 1000
for: 5m
```

**Action:** Check network connectivity, peer health, and sync status

#### 2. TestnetNodePeerCountLow
**Trigger:** Peer count drops below minimum (25)  
**Severity:** CRITICAL  
**Expression:**
```promql
neo_peer_count < 25
for: 5m
```

**Action:** Review P2P configuration, check seed nodes, verify firewall settings

#### 3. TestnetNodeStateRootValidationFailure
**Trigger:** State root validation failures detected  
**Severity:** CRITICAL  
**Expression:**
```promql
rate(neo_state_roots_rejected_total[1h]) > 0
for: 5m
```

**Action:** Investigate state divergence, consider resync from clean snapshot

#### 4. TestnetNodePrefetchPipelineStalled
**Trigger:** Prefetch pipeline not making progress  
**Severity:** CRITICAL  
**Expression:**
```promql
rate(prefetch_stage_latencies{stage="execute"}[5m]) == 0 
and neo_sync_current_height increases[5m] == 0
for: 10m
```

**Action:** Check system resources, review optimization logs

#### 5. TestnetNodeMemoryHigh
**Trigger:** Memory usage > 90%  
**Severity:** CRITICAL  
**Expression:**
```promql
memory_usage_bytes / system_memory_total_bytes * 100 > 90
for: 10m
```

**Action:** Monitor disk space, consider increasing cache size or adding RAM

### Warning Alerts

#### 1. TestnetNodeFastSyncActive
**Trigger:** Fast sync mode active for >15 minutes  
**Severity:** WARNING  
**Expression:**
```promql
neo_fast_sync_enabled == 1
for: 15m
```

**Action:** Normal during initial sync; investigate if persistent after full sync

#### 2. TestnetNodeBlockProcessingSlow
**Trigger:** Block processing latency elevated (>2s p99)  
**Severity:** WARNING  
**Expression:**
```promql
histogram_quantile(0.99, rate(block_processing_duration_bucket[5m])) > 2
for: 10m
```

**Action:** Check system resources, review optimization effectiveness

#### 3. TestnetNodeSyscallLatencySpiked
**Trigger:** Syscall lookup latency > 500ns average  
**Severity:** WARNING  
**Expression:**
```promql
histogram_quantile(0.95, rate(get_syscall_lookup_latency_bucket[5m])) > 500e-9
for: 10m
```

**Action:** Verify Cuckoo hash table is working correctly, check cache hit rates

#### 4. TestnetNodeDiskSpaceLow
**Trigger:** Disk free space < 15%  
**Severity:** WARNING  
**Expression:**
```promql
neo_storage_free_bytes / neo_storage_total_bytes * 100 < 15
for: 30m
```

**Action:** Free up disk space, archive old logs, consider expanding storage

### Info Level Alerts

#### 1. TestnetNodeCuckooHashLowHitRate
**Trigger:** Hit rate < 70% for 30 minutes  
**Severity:** INFO  
**Expression:**
```promql
(sum(rate(neo_cuckoo_hash_lookups_total[1h])) 
 / (sum(rate(neo_cuckoo_hash_lookups_total[1h])) 
 + sum(rate(neo_cuckoo_hash_lookups_miss_total[1h]))) * 100) < 70
for: 30m
```

**Action:** Monitor trend; may indicate suboptimal cache sizing

#### 2. TestnetNodeMempoolGrowing
**Trigger:** Mempool size > 40,000 transactions for 30 minutes  
**Severity:** INFO  
**Expression:**
```promql
neo_mempool_size > 40000
for: 30m
```

**Action:** Monitor for congestion; normal during high activity periods

## Query Examples

### Performance Analysis

**Average TPS over last hour:**
```promql
avg_over_time(rate(neo_transactions_verified_total[1h])[1h:])
```

**Current sync speed (blocks/sec):**
```promql
rate(neo_blocks_processed_total[1m])
```

**Network bandwidth utilization:**
```promql
# Requires external network metrics
rate(node_network_receive_bytes_total[5m]) / 1048576
```

**Optimization effectiveness comparison:**

Before vs After cuckoo hash:
```promql
# Current syscall latency
histogram_quantile(0.95, rate(get_syscall_lookup_latency_bucket[5m]))

# Historical comparison (add time range parameter)
histogram_quantile(0.95, rate(get_syscall_lookup_latency_bucket[1h][1h]))
```

**Batch verification efficiency:**
```promql
# Signatures verified per second
rate(neo_batch_signatures_verified_total[5m])

# Batches submitted per second
rate(processed_batches_total[5m])

# Average per batch
rate(neo_batch_signatures_verified_total[5m]) 
/ rate(processed_batches_total[5m])
```

### Troubleshooting Queries

**Identify slow blocks:**
```promql
# Find blocks taking >2 seconds to process
topk(10, 
  sort_desc(
    histogram_quantile(0.99, rate(block_processing_duration_bucket[5m]))
  )
)
```

**Monitor prefetch pipeline health:**
```promql
# Should be >0 for healthy pipeline
prefetch_stage_latencies_bucket{stage="execute"}

# Compare stages for bottleneck
prefetch_stage_latencies_bucket{stage="io"}
-
prefetch_stage_latencies_bucket{stage="verify"}
```

**Check resource pressure:**
```promql
# High memory pressure
memory_usage_bytes > system_memory_total_bytes * 0.85

# CPU saturation
cpu_usage_ratio > 0.95
```

## Deployment Instructions

### 1. Install Prometheus

```bash
# Download Prometheus (adjust for your platform)
wget https://github.com/prometheus/prometheus/releases/download/v2.47.0/prometheus-2.47.0.linux-amd64.tar.gz
tar xvfz prometheus-2.47.0.linux-amd64.tar.gz
cd prometheus-2.47.0.linux-amd64/

# Copy configuration
cp ../../config/prometheus/prometheus.yml .

# Start Prometheus
./prometheus \
  --config.file=prometheus.yml \
  --storage.tsdb.path=/var/lib/prometheus \
  --web.enable-lifecycle
```

### 2. Configure Alertmanager

```bash
# Download Alertmanager
wget https://github.com/prometheus/alertmanager/releases/download/v0.26.0/alertmanager-0.26.0.linux-amd64.tar.gz
tar xvfz alertmanager-0.26.0.linux-amd64.tar.gz
cd alertmanager-0.26.0.linux-amd64/

# Create templates directory
mkdir -p html_templates alerts

# Copy configuration
cp ../../config/prometheus/alertmanager.yml .

# Set environment variables
export SMTP_PASSWORD="your-smtp-password"
export SLACK_WEBHOOK_URL="your-slack-webhook-url"

# Start Alertmanager
./alertmanager \
  --config.file=alertmanager.yml \
  --storage.path=/var/lib/alertmanager \
  --cluster.listen-address=""
```

### 3. Install Grafana

```bash
# Ubuntu/Debian
sudo apt-get install -y adduser libfontconfig1
wget https://dl.grafana.com/enterprise/release/grafana-enterprise_10.0.0_amd64.deb
sudo dpkg -i grafana-enterprise_10.0.0_amd64.deb
sudo systemctl start grafana-server

# Add Prometheus data source
curl -X POST http://localhost:3000/api/datasources \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Prometheus",
    "type": "prometheus",
    "url": "http://localhost:9090",
    "access": "proxy",
    "isDefault": true
  }'

# Import dashboard
curl -X POST http://localhost:3000/api/dashboards/db \
  -H "Content-Type: application/json" \
  --data-binary @../../config/grafana/neo-node-dashboard.json
```

### 4. Configure Firewall

Allow necessary ports:
```bash
# Prometheus metrics endpoint
iptables -A INPUT -p tcp --dport 9090 -s 127.0.0.1 -j ACCEPT

# Grafana web interface
iptables -A INPUT -p tcp --dport 3000 -s 0.0.0.0/0 -j ACCEPT

# Prometheus scraping port (internal)
iptables -A INPUT -p tcp --dport 9093 -s 127.0.0.1 -j ACCEPT
```

## Log Rotation Configuration

The log rotation is configured in `testnet-production.toml`:

```toml
[monitoring.logging]
file_path = "./logs/neo-node-testnet.log"
max_file_size = "100MB"
max_files = 10
```

This keeps 1GB of logs total (10 files × 100MB each).

### Manual Log Management

```bash
# Rotate logs manually
mv ./logs/neo-node-testnet.log ./logs/neo-node-testnet.log.$(date +%Y%m%d_%H%M%S)

# Clear old logs (>30 days)
find ./logs -name "*.log.*" -mtime +30 -delete

# Check current log size
du -sh ./logs/neo-node-testnet.log
```

## Monitoring Checklist

### Daily Checks
- [ ] Review Grafana dashboard for anomalies
- [ ] Check critical alerts (if any)
- [ ] Verify sync progress (lag < 100 blocks)
- [ ] Confirm peer count stable (> 25 peers)

### Weekly Checks
- [ ] Analyze optimization effectiveness (cuckoo hit rate > 70%)
- [ ] Review system resource trends (CPU, memory, disk)
- [ ] Check log rotation is working correctly
- [ ] Update metrics baseline if needed

### Monthly Reviews
- [ ] Review alert rules and adjust thresholds
- [ ] Analyze long-term performance trends
- [ ] Optimize cache sizes based on workload
- [ ] Review and update runbooks as needed

## Troubleshooting Guide

### Symptom: No metrics appearing in Prometheus

**Cause:** Prometheus cannot reach the node's metrics endpoint

**Solution:**
```bash
# Check if metrics endpoint is responding
curl -v http://127.0.0.1:9090/metrics

# Verify firewall allows connection
iptables -L -n | grep 9090

# Check Prometheus scrape logs
tail -f /var/log/prometheus/prometheus.log | grep neo-node
```

### Symptom: Alerts not firing

**Cause:** Alertmanager not receiving metrics or incorrect expression

**Solution:**
```bash
# Test PromQL query directly
curl "http://localhost:9090/api/v1/query?query=test"

# Check Alertmanager status
curl http://localhost:9093/api/v1/status

# Verify rule files loaded
curl http://localhost:9090/-/rules | grep -A5 neo-sync
```

### Symptom: High header lag during sync

**Cause:** Network issues or system resource constraints

**Solution:**
```bash
# Check peer connectivity
curl http://localhost:9090/healthz | jq '.peer_count'

# Monitor system resources
htop

# Check prefetch pipeline
watch -n 5 'curl -s http://localhost:9090/metrics | grep prefetch'

# Consider enabling fast sync temporarily
# This is automatic but can be verified via metric
curl -s http://localhost:9090/metrics | grep neo_fast_sync_enabled
```

### Symptom: Low Cuckoo hash hit rate

**Cause:** Cache not sized appropriately for workload

**Solution:**
```bash
# Check current cache configuration
grep lru_cache_size config/testnet-production.toml

# If consistently low (<50%), increase cache size
# Edit config and restart node:
# lru_cache_size_mb = 1024  # Increase from 512MB

# Monitor hit rate improvement
# Query in Grafana or via PromQL
(sum(rate(neo_cuckoo_hash_lookups_total[1h])) 
 / (sum(rate(neo_cuckoo_hash_lookups_total[1h])) 
 + sum(rate(neo_cuckoo_hash_lookups_miss_total[1h]))) * 100)
```

## Security Considerations

### Exposed Endpoints

The following endpoints are exposed locally:

- `http://127.0.0.1:9090/metrics` - Prometheus metrics
- `http://127.0.0.1:9090/healthz` - Liveness probe
- `http://127.0.0.1:9090/ready` - Readiness probe

**Important:** These are bound to localhost only. Do not expose to external networks without proper authentication and authorization.

### Authentication

Currently no authentication is implemented on these endpoints. For production deployments:

1. Use network isolation (firewall, VPC)
2. Implement reverse proxy with auth
3. Enable TLS termination at load balancer

### Data Privacy

Metrics do not contain sensitive information. However:

- Block heights and transaction counts reveal network activity
- Latency measurements can be used for timing attacks
- Always scrape metrics internally, never expose publicly

## Maintenance Procedures

### Rolling Restart

To update metrics collection without downtime:

```bash
# 1. Reload Prometheus configuration
curl -X POST http://localhost:9090/-/reload

# 2. Verify new configuration loaded
curl http://localhost:9090/api/v1/status

# 3. Graceful restart if needed
systemctl restart prometheus
```

### Backup Procedures

**Prometheus TSDB:**
```bash
# Stop Prometheus
systemctl stop prometheus

# Backup data directory
tar czf /backup/prometheus-backup-$(date +%Y%m%d).tar.gz /var/lib/prometheus

# Restart
systemctl start prometheus
```

**Grafana Dashboards:**
```bash
# Export dashboards via API
curl -u admin:password http://localhost:3000/api/search > dashboards.json

# Or export specific dashboard
curl -u admin:password http://localhost:3000/api/dashboards/db/neo-prod-monitoring > neo-dashboard.json
```

## Contact & Support

For issues or questions:

- **Repository:** https://github.com/r3e-network/neo-rs
- **Documentation:** docs/MONITORING.md
- **Issues:** GitHub Issues section

## Version History

- **v1.0 (2026-09-14):** Initial comprehensive monitoring setup
  - Prometheus metrics integration
  - Alert rules configuration
  - Grafana dashboard creation
  - Production-grade health endpoints
  
- **v0.17.0+:** Aligned with Neo-RS Phase 1-3 optimizations

---

**Last Updated:** September 14, 2026  
**Maintainer:** R3E Network Operations Team
