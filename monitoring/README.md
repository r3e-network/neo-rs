# Neo N3 Monitoring Infrastructure

This directory contains the complete monitoring setup for Neo N3 blockchain nodes, including Prometheus metrics collection, Grafana dashboards, and alerting rules.

## Directory Structure

```
monitoring/
├── prometheus/
│   ├── prometheus.yml           # Prometheus server configuration
│   └── alerts/
│       ├── neo-performance.yml  # Performance-related alerts
│       ├── neo-resources.yml    # Resource utilization alerts
│       └── neo-blockchain.yml   # Blockchain health alerts
├── grafana/
│   └── dashboards/
│       ├── throughput.json           # Real-time throughput dashboard
│       ├── latency.json              # Latency distribution dashboard
│       ├── cache-performance.json    # Cache performance dashboard
│       └── resource-utilization.json # Resource utilization dashboard
└── alertmanager/
    └── alertmanager.yml         # Alert routing and notification configuration
```

## Metrics Overview

### Performance Metrics

#### Throughput Metrics
- `neo_blocks_per_second` - Current blocks per second processing rate
- `neo_transactions_per_second` - Current transactions per second processing rate
- `neo_state_operations_per_second` - Current state operations per second

#### Latency Metrics
- `neo_operation_latency_seconds` - Histogram for p50/p95/p99 latency tracking
- `block_processing_duration` - Block processing time histogram
- `neo_ledger_tx_verification_seconds` - Transaction verification time
- `neo_vm_execution_seconds` - VM execution time

#### Cache Metrics
- `neo_cache_hit_rate` - Cache hit rate ratio (0.0 to 1.0)
- `neo_storage_cache_hits_total` - Total storage cache hits
- `neo_storage_cache_misses_total` - Total storage cache misses
- `neo_cuckoo_hash_lookups_total` - Cuckoo hash successful lookups
- `neo_cuckoo_hash_lookups_miss_total` - Cuckoo hash failed lookups

#### Resource Metrics
- `neo_memory_usage_bytes` - Memory usage in bytes
- `neo_system_memory_total_bytes` - Total system memory
- `neo_cpu_utilization` - CPU utilization ratio (0.0 to 1.0)
- `neo_storage_free_bytes` - Free disk space
- `neo_storage_total_bytes` - Total disk space

## Grafana Dashboards

### 1. Real-time Throughput Dashboard
**File**: `grafana/dashboards/throughput.json`

Displays:
- Blocks per second with warning/critical thresholds
- Transactions per second
- State operations per second
- Mempool size and composition
- Block height progress

**Alerts**:
- Blocks/sec < 700 (warning)
- Blocks/sec < 500 (critical)

### 2. Latency Distribution Dashboard
**File**: `grafana/dashboards/latency.json`

Displays:
- Operation latency percentiles (P50, P95, P99)
- Current P50/P95/P99 latency stats
- Block processing time distribution
- Transaction verification time
- VM execution time
- Consensus round time

**Alerts**:
- P99 latency > 100ms (warning)

### 3. Cache Performance Dashboard
**File**: `grafana/dashboards/cache-performance.json`

Displays:
- Overall cache hit rate
- Storage cache hit rate
- Cache operations rate (hits/misses per second)
- Cuckoo hash performance
- Storage I/O operations

**Alerts**:
- Cache hit rate < 80% (warning)

### 4. Resource Utilization Dashboard
**File**: `grafana/dashboards/resource-utilization.json`

Displays:
- CPU utilization with gauges
- Memory usage percentage and trends
- Disk space usage and trends
- Storage I/O throughput
- Network peer count
- P2P connection statistics
- Network throughput

**Alerts**:
- CPU > 90% (warning)
- Memory > 90% (critical)

## Alerting Rules

### Performance Alerts (`alerts/neo-performance.yml`)

| Alert | Condition | Duration | Severity | Description |
|-------|-----------|----------|----------|-------------|
| `BlockProcessingSlowWarning` | blocks/sec < 700 | 2m | warning | Block processing below target |
| `BlockProcessingSlowCritical` | blocks/sec < 500 | 1m | critical | Block processing critically low |
| `HighLatencyWarning` | P99 > 100ms | 5m | warning | High operation latency |
| `LowCacheHitRate` | hit rate < 80% | 5m | warning | Poor cache performance |
| `LowTransactionThroughput` | tx/sec < 100 | 5m | warning | Low transaction processing |
| `StateOperationsSlowdown` | ops/sec < 1000 | 5m | warning | Slow state operations |

### Resource Alerts (`alerts/neo-resources.yml`)

| Alert | Condition | Duration | Severity | Description |
|-------|-----------|----------|----------|-------------|
| `HighMemoryUsage` | memory > 90% | 2m | critical | Risk of OOM |
| `MemoryUsageHigh` | memory > 80% | 5m | warning | High memory usage |
| `HighCPUUtilization` | CPU > 90% | 5m | warning | High CPU load |
| `DiskSpaceLow` | free < 20% | 5m | warning | Low disk space |
| `DiskSpaceCritical` | free < 10% | 2m | critical | Critically low disk space |
| `LowPeerCount` | peers < 3 | 5m | warning | Network connectivity issues |
| `NoPeers` | peers = 0 | 2m | critical | Node isolated |

### Blockchain Alerts (`alerts/neo-blockchain.yml`)

| Alert | Condition | Duration | Severity | Description |
|-------|-----------|----------|----------|-------------|
| `BlockSyncStalled` | no height increase | 5m | critical | Block sync stopped |
| `HighHeaderLag` | lag > 100 blocks | 5m | warning | Slow block processing |
| `MempoolSaturation` | size > 5000 tx | 10m | warning | High mempool load |
| `StateRootValidationLag` | lag > 50 blocks | 10m | warning | State validation delayed |
| `HighVMFaultRate` | faults > 10/s | 5m | warning | Smart contract issues |
| `ConsensusViewChangesFrequent` | view changes > 5/s | 5m | warning | Consensus instability |
| `OracleRequestBacklog` | pending > 100 | 10m | warning | Oracle service issues |
| `HighRPCRateLimiting` | rate limited > 10/s | 5m | warning | RPC overload |

## Deployment

### Using Docker Compose

```yaml
version: '3.8'

services:
  prometheus:
    image: prom/prometheus:latest
    ports:
      - "9090:9090"
    volumes:
      - ./monitoring/prometheus/prometheus.yml:/etc/prometheus/prometheus.yml
      - ./monitoring/prometheus/alerts:/etc/prometheus/alerts
      - prometheus-data:/prometheus
    command:
      - '--config.file=/etc/prometheus/prometheus.yml'
      - '--storage.tsdb.path=/prometheus'
      - '--web.console.libraries=/usr/share/prometheus/console_libraries'
      - '--web.console.templates=/usr/share/prometheus/consoles'

  alertmanager:
    image: prom/alertmanager:latest
    ports:
      - "9093:9093"
    volumes:
      - ./monitoring/alertmanager/alertmanager.yml:/etc/alertmanager/alertmanager.yml
      - alertmanager-data:/alertmanager
    command:
      - '--config.file=/etc/alertmanager/alertmanager.yml'
      - '--storage.path=/alertmanager'

  grafana:
    image: grafana/grafana:latest
    ports:
      - "3000:3000"
    volumes:
      - ./monitoring/grafana/dashboards:/etc/grafana/provisioning/dashboards
      - grafana-data:/var/lib/grafana
    environment:
      - GF_SECURITY_ADMIN_PASSWORD=admin
      - GF_USERS_ALLOW_SIGN_UP=false

  neo-node:
    # Your Neo node configuration
    ports:
      - "9090:9090"  # Expose metrics endpoint

volumes:
  prometheus-data:
  alertmanager-data:
  grafana-data:
```

### Manual Deployment

1. **Install Prometheus**:
   ```bash
   # Download and extract Prometheus
   wget https://github.com/prometheus/prometheus/releases/download/v2.45.0/prometheus-2.45.0.linux-amd64.tar.gz
   tar xvfz prometheus-*.tar.gz
   cd prometheus-*
   
   # Copy configuration
   cp /path/to/monitoring/prometheus/prometheus.yml ./prometheus.yml
   cp -r /path/to/monitoring/prometheus/alerts ./alerts
   
   # Start Prometheus
   ./prometheus --config.file=prometheus.yml
   ```

2. **Install Alertmanager**:
   ```bash
   # Download and extract Alertmanager
   wget https://github.com/prometheus/alertmanager/releases/download/v0.26.0/alertmanager-0.26.0.linux-amd64.tar.gz
   tar xvfz alertmanager-*.tar.gz
   cd alertmanager-*
   
   # Copy configuration
   cp /path/to/monitoring/alertmanager/alertmanager.yml ./alertmanager.yml
   
   # Start Alertmanager
   ./alertmanager --config.file=alertmanager.yml
   ```

3. **Install Grafana**:
   ```bash
   # Install Grafana (Ubuntu/Debian)
   sudo apt-get install -y adduser libfontconfig1
   wget https://dl.grafana.com/enterprise/release/grafana-enterprise_10.0.0_amd64.deb
   sudo dpkg -i grafana-enterprise_10.0.0_amd64.deb
   
   # Start Grafana
   sudo systemctl start grafana-server
   sudo systemctl enable grafana-server
   ```

4. **Import Dashboards**:
   - Navigate to Grafana (http://localhost:3000)
   - Login (default: admin/admin)
   - Go to Dashboards → Import
   - Upload each JSON file from `monitoring/grafana/dashboards/`

## Configuration

### Prometheus Metrics Endpoint

Ensure your Neo node exposes metrics on the configured port (default: 9090):

```rust
// In your neo-node startup code
use neo_telemetry::{init_for_node, TelemetryAndLoggingConfig};

let config = TelemetryAndLoggingConfig::default();
let (telemetry_handle, _guard) = init_for_node(&config, false)?;

// Metrics are automatically exposed via the configured endpoint
```

### Customizing Alert Thresholds

Edit the alert files in `monitoring/prometheus/alerts/` to adjust thresholds:

```yaml
# Example: Change block processing warning threshold
- alert: BlockProcessingSlowWarning
  expr: neo_blocks_per_second < 500  # Changed from 700
  for: 2m
```

### Notification Channels

Edit `monitoring/alertmanager/alertmanager.yml` to configure notification channels:

```yaml
receivers:
  - name: 'neo-critical'
    email_configs:
      - to: 'oncall@your-domain.com'
        from: 'alerts@your-domain.com'
        smarthost: 'smtp.gmail.com:587'
    slack_configs:
      - api_url: 'https://hooks.slack.com/services/YOUR/WEBHOOK/URL'
        channel: '#neo-critical'
    pagerduty_configs:
      - service_key: 'YOUR_PAGERDUTY_KEY'
```

## Accessing Dashboards

- **Prometheus**: http://localhost:9090
- **Alertmanager**: http://localhost:9093
- **Grafana**: http://localhost:3000

## Maintenance

### Retention Policies

Prometheus default retention is 15 days. Adjust with:

```bash
./prometheus --storage.tsdb.retention.time=30d
```

### Backup

Back up Prometheus data regularly:

```bash
# Stop Prometheus
# Copy data directory
tar -czf prometheus-backup-$(date +%Y%m%d).tar.gz /path/to/prometheus/data
# Restart Prometheus
```

## Troubleshooting

### Metrics Not Appearing

1. Verify Neo node metrics endpoint is accessible:
   ```bash
   curl http://localhost:9090/metrics | grep neo_
   ```

2. Check Prometheus targets:
   - Navigate to http://localhost:9090/targets
   - Ensure "neo-node" target is UP

3. Review Prometheus logs for scrape errors

### Alerts Not Firing

1. Check alert rules syntax:
   ```bash
   promtool check rules /path/to/alerts/*.yml
   ```

2. Verify Alertmanager is receiving alerts:
   - Navigate to http://localhost:9093
   - Check "Alerts" section

3. Test alert routing:
   ```bash
   amtool alert --alertmanager.url=http://localhost:9093
   ```

## Performance Impact

The monitoring infrastructure has minimal performance impact:
- Prometheus scraping: ~0.1% CPU overhead
- Metrics collection: ~10MB memory overhead
- Storage: ~1GB per week of metrics data (default retention)

## License

MIT License - See root LICENSE file for details.
