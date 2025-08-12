# Neo Rust Health Check API Documentation

## Overview

The Neo Rust node provides comprehensive health check endpoints for monitoring node status, performance, and reliability. These endpoints are designed to integrate with various monitoring systems including Kubernetes, Prometheus, and custom monitoring solutions.

## 🔍 Available Endpoints

### 1. Simple Health Check

**Endpoint:** `GET /health`

**Description:** Basic health check that returns node status with minimal overhead.

**Response Codes:**
- `200 OK` - Node is healthy
- `503 Service Unavailable` - Node is unhealthy

**Response Example:**
```json
{
  "status": "healthy",
  "block_height": 1234567,
  "peer_count": 8
}
```

**Usage:**
```bash
curl -f http://localhost:8080/health || echo "Node unhealthy"
```

### 2. Liveness Probe

**Endpoint:** `GET /health/live`

**Description:** Kubernetes liveness probe endpoint. Indicates if the process is alive and should not be restarted.

**Response Codes:**
- `200 OK` - Process is alive

**Response Example:**
```json
{
  "alive": true,
  "timestamp": 1640000000
}
```

**Kubernetes Configuration:**
```yaml
livenessProbe:
  httpGet:
    path: /health/live
    port: 8080
  initialDelaySeconds: 30
  periodSeconds: 10
```

### 3. Readiness Probe

**Endpoint:** `GET /health/ready`

**Description:** Kubernetes readiness probe endpoint. Indicates if the node is ready to accept traffic.

**Response Codes:**
- `200 OK` - Node is ready
- `503 Service Unavailable` - Node is not ready

**Response Example:**
```json
{
  "ready": true,
  "reason": null
}
```

**Not Ready Example:**
```json
{
  "ready": false,
  "reason": "No peer connections"
}
```

**Kubernetes Configuration:**
```yaml
readinessProbe:
  httpGet:
    path: /health/ready
    port: 8080
  initialDelaySeconds: 10
  periodSeconds: 5
```

### 4. Detailed Health Check

**Endpoint:** `GET /health/detailed`

**Description:** Comprehensive health check with detailed diagnostics for all subsystems.

**Response Codes:**
- `200 OK` - Node is healthy or degraded
- `503 Service Unavailable` - Node is unhealthy

**Response Example:**
```json
{
  "status": "healthy",
  "timestamp": 1640000000,
  "uptime_seconds": 86400,
  "version": "1.0.0",
  "checks": {
    "database": {
      "status": true,
      "message": "Database is operational",
      "duration_ms": 5
    },
    "network": {
      "status": true,
      "message": "Connected to 8 peers",
      "duration_ms": 2
    },
    "consensus": {
      "status": true,
      "message": "Not a consensus node",
      "duration_ms": 1
    },
    "rpc": {
      "status": true,
      "message": "RPC is responsive",
      "duration_ms": 3
    },
    "sync": {
      "status": true,
      "message": "Fully synced (lag: 0 blocks)",
      "duration_ms": 4
    },
    "resources": {
      "status": true,
      "message": "Resources within normal range",
      "duration_ms": 2
    }
  },
  "metrics": {
    "block_height": 1234567,
    "peer_count": 8,
    "mempool_size": 42,
    "sync_progress": 100.0,
    "last_block_time": 1640000000,
    "transactions_per_second": 15.5
  }
}
```

## 📊 Health States

### Status Values

| Status | Description | Monitoring Action |
|--------|-------------|-------------------|
| `healthy` | All systems operational | None required |
| `degraded` | Non-critical issues present | Monitor closely |
| `unhealthy` | Critical issues detected | Immediate attention |

### Check Results

Each subsystem check returns:
- `status`: Boolean indicating pass/fail
- `message`: Human-readable description
- `duration_ms`: Time taken for the check

## 🔧 Configuration

### Enabling Health Endpoints

Add to your node configuration:

```toml
[health]
enabled = true
bind_address = "0.0.0.0:8080"
check_interval = 30  # seconds

[health.thresholds]
max_peer_lag = 10  # blocks
min_peers = 3
max_memory_percent = 80
max_cpu_percent = 80
max_disk_percent = 90
```

### Custom Health Checks

You can add custom health checks:

```rust
// Implement custom check
impl HealthCheck for MyCustomCheck {
    async fn check(&self) -> CheckResult {
        // Your check logic
        CheckResult {
            status: true,
            message: "Custom check passed".to_string(),
            duration_ms: 10,
        }
    }
}

// Register with health service
health_service.add_check("custom", Box::new(MyCustomCheck));
```

## 🚀 Integration Examples

### Prometheus Integration

```yaml
# prometheus.yml
scrape_configs:
  - job_name: 'neo-health'
    metrics_path: '/health/detailed'
    scrape_interval: 30s
    static_configs:
      - targets: ['localhost:8080']
```

### Grafana Alert

```json
{
  "alert": "Neo Node Unhealthy",
  "expr": "up{job=\"neo-health\"} == 0",
  "for": "5m",
  "annotations": {
    "summary": "Neo node health check failing"
  }
}
```

### Docker Healthcheck

```dockerfile
HEALTHCHECK --interval=30s --timeout=10s --start-period=60s \
  CMD curl -f http://localhost:8080/health || exit 1
```

### Kubernetes Deployment

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: neo-node
spec:
  template:
    spec:
      containers:
      - name: neo
        image: neo-rust:latest
        ports:
        - containerPort: 8080
          name: health
        livenessProbe:
          httpGet:
            path: /health/live
            port: health
          initialDelaySeconds: 30
          periodSeconds: 10
          timeoutSeconds: 5
          failureThreshold: 3
        readinessProbe:
          httpGet:
            path: /health/ready
            port: health
          initialDelaySeconds: 10
          periodSeconds: 5
          timeoutSeconds: 3
          failureThreshold: 3
```

### Load Balancer Health Check

**HAProxy:**
```
backend neo-nodes
    option httpchk GET /health
    http-check expect status 200
    server node1 10.0.0.1:8080 check
    server node2 10.0.0.2:8080 check
```

**NGINX:**
```nginx
upstream neo_nodes {
    server 10.0.0.1:8080 max_fails=3 fail_timeout=30s;
    server 10.0.0.2:8080 max_fails=3 fail_timeout=30s;
}

location /health_check {
    proxy_pass http://neo_nodes/health;
    health_check interval=10s fails=3 passes=2;
}
```

## 📈 Monitoring Best Practices

### 1. Check Frequency

| Check Type | Recommended Interval | Timeout |
|------------|---------------------|---------|
| Liveness | 30 seconds | 10 seconds |
| Readiness | 10 seconds | 5 seconds |
| Detailed | 60 seconds | 30 seconds |

### 2. Alerting Thresholds

```yaml
alerts:
  - name: NodeUnhealthy
    condition: status == "unhealthy"
    duration: 5m
    severity: critical
    
  - name: NodeDegraded
    condition: status == "degraded"
    duration: 15m
    severity: warning
    
  - name: LowPeerCount
    condition: peer_count < 3
    duration: 10m
    severity: warning
    
  - name: SyncLag
    condition: sync_progress < 99
    duration: 30m
    severity: warning
```

### 3. Dashboard Metrics

Key metrics to display:
- Health status over time
- Individual check success rates
- Response time percentiles
- Resource usage trends
- Sync progress

## 🔍 Troubleshooting

### Common Issues

**Health check timeout:**
```bash
# Increase timeout
curl --max-time 30 http://localhost:8080/health/detailed
```

**False positives:**
```bash
# Check individual components
curl http://localhost:8080/health/detailed | jq '.checks'
```

**Resource constraints:**
```bash
# Monitor during health checks
htop
iostat -x 1
```

### Debug Mode

Enable debug logging for health checks:

```toml
[logging]
level = "debug"
modules = ["health"]
```

## 📚 API Reference

### Response Schemas

All responses follow these TypeScript interfaces:

```typescript
interface SimpleHealth {
  status: "healthy" | "unhealthy";
  block_height: number;
  peer_count: number;
}

interface LivenessProbe {
  alive: boolean;
  timestamp: number;
}

interface ReadinessProbe {
  ready: boolean;
  reason?: string;
}

interface HealthStatus {
  status: "healthy" | "degraded" | "unhealthy";
  timestamp: number;
  uptime_seconds: number;
  version: string;
  checks: HealthChecks;
  metrics: HealthMetrics;
}
```

## 🛡️ Security Considerations

1. **Access Control**: Health endpoints should be accessible only from trusted sources
2. **Rate Limiting**: Implement rate limiting to prevent DoS
3. **Information Disclosure**: Detailed endpoint may expose sensitive information
4. **Network Isolation**: Consider running health checks on internal network only

---

For additional monitoring setup, see the [Monitoring Guide](./MONITORING_SETUP.md).