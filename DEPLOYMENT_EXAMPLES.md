# Neo-N3 Production Monitoring - Deployment Examples

## Quick Start Options

Choose your preferred deployment method below. All options deploy the complete monitoring stack with Prometheus, Alertmanager, and Grafana.

---

## Option 1: Docker Compose (Recommended)

### Prerequisites
- Docker installed (v20+ recommended)
- Docker Compose plugin or standalone
- Git repository cloned
- At least 4GB RAM available

### Steps

```bash
# 1. Copy environment template
cp .env.example .env

# 2. Edit .env with your credentials
#   SMTP_PASSWORD=your-smtp-password
#   SLACK_WEBHOOK_URL=https://hooks.slack.com/services/YOUR/WEBHOOK/URL
#   GRAFANA_ADMIN_PASSWORD=secure-admin-password

# 3. Start the monitoring stack
docker-compose -f monitoring-docker-compose.yml --env-file .env up -d

# 4. Check services are running
docker-compose -f monitoring-docker-compose.yml ps

# 5. View logs (optional)
docker-compose -f monitoring-docker-compose.yml logs -f
```

### Access Services

| Service | URL | Default Credentials |
|---------|-----|---------------------|
| Neo Node Metrics | http://localhost:9090/metrics | N/A |
| Health Check | http://localhost:9090/healthz | N/A |
| Prometheus | http://localhost:9090 | N/A |
| Alertmanager | http://localhost:9093 | N/A |
| Grafana | http://localhost:3000 | admin / PASSWORD_FROM_ENV |

### Stop Services

```bash
# Graceful shutdown
docker-compose -f monitoring-docker-compose.yml down

# With volume cleanup (data will be lost)
docker-compose -f monitoring-docker-compose.yml down -v
```

### Update Configuration

Edit configuration files in `config/` directory, then restart:

```bash
# Restart only Prometheus with new config
docker exec prometheus kill -HUP $(cat /proc/self/comm)

# Or simply restart all services
docker-compose -f monitoring-docker-compose.yml restart
```

---

## Option 2: Manual Installation (Linux)

### Prerequisites
- Linux system (Ubuntu/Debian tested)
- wget, tar, curl installed
- Systemd init system
- Minimum 2GB free disk space
- Python 3 for setup script

### Automated Setup Script

```bash
# Make script executable
chmod +x scripts/setup-monitoring.sh

# Set environment variables (optional)
export SMTP_PASSWORD="your-smtp-password"
export SLACK_WEBHOOK_URL="https://hooks.slack.com/services/YOUR/WEBHOOK/URL"
export PAGERDUTY_SERVICE_KEY="your-pagerduty-key"

# Run setup
./scripts/setup-monitoring.sh
```

### Verify Installation

```bash
# Check service status
systemctl status prometheus
systemctl status alertmanager

# Test Prometheus metrics endpoint
curl http://localhost:9090/metrics | grep neo_sync

# Test health endpoints
curl http://localhost:9090/healthz
curl http://localhost:9090/ready

# Import Grafana dashboard (manual)
# Open http://localhost:3000
# Navigate to Dashboards > Import
# Upload config/grafana/neo-node-dashboard.json
```

### Manual Component Installation

If you prefer individual installation:

#### Install Prometheus
```bash
wget https://github.com/prometheus/prometheus/releases/download/v2.47.0/prometheus-2.47.0.linux-amd64.tar.gz
tar xvfz prometheus-2.47.0.linux-amd64.tar.gz
cd prometheus-2.47.0.linux-amd64/

cp prometheus promtool /usr/local/bin/
cp ../config/prometheus/prometheus.yml /etc/prometheus/

# Create systemd service
sudo tee /etc/systemd/system/prometheus.service <<EOF
[Unit]
Description=Prometheus
After=network.target

[Service]
ExecStart=/usr/local/bin/prometheus \
    --config.file=/etc/prometheus/prometheus.yml \
    --storage.tsdb.path=/var/lib/prometheus
Restart=on-failure

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable prometheus
sudo systemctl start prometheus
```

#### Install Alertmanager
```bash
wget https://github.com/prometheus/alertmanager/releases/download/v0.26.0/alertmanager-0.26.0.linux-amd64.tar.gz
tar xvfz alertmanager-0.26.0.linux-amd64.tar.gz
cd alertmanager-0.26.0.linux-amd64/

cp alertmanager amtool /usr/local/bin/
cp ../config/prometheus/alertmanager.yml /etc/alertmanager/

# Create systemd service
sudo tee /etc/systemd/system/alertmanager.service <<EOF
[Unit]
Description=Alertmanager
After=network.target

[Service]
Environment="SMTP_PASSWORD=${SMTP_PASSWORD:-}"
Environment="SLACK_WEBHOOK_URL=${SLACK_WEBHOOK_URL:-}"
ExecStart=/usr/local/bin/alertmanager \
    --config.file=/etc/alertmanager/alertmanager.yml \
    --storage.path=/var/lib/alertmanager
Restart=on-failure

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable alertmanager
sudo systemctl start alertmanager
```

#### Install Grafana
```bash
# Add Grafana repository (Ubuntu/Debian)
wget -q -O - https://packages.grafana.com/gpg.key | sudo apt-key add -
echo "deb https://packages.grafana.com/oss/deb stable main" | sudo tee /etc/apt/sources.list.d/grafana.list

sudo apt-get update
sudo apt-get install -y grafana

sudo systemctl enable grafana-server
sudo systemctl start grafana-server

# Import dashboard via API
curl -X POST http://localhost:3000/api/dashboards/db \
    -H "Content-Type: application/json" \
    -d @config/grafana/neo-node-dashboard.json \
    -u admin:admin
```

---

## Option 3: Verify Existing Node

If your Neo node is already running, you can verify its metrics without deploying full stack:

### Quick Verification

```bash
# 1. Run verification script
chmod +x scripts/verify-monitoring.sh
./scripts/verify-monitoring.sh

# Manual checks:
curl http://127.0.0.1:9090/metrics | head -n 50
curl http://127.0.0.1:9090/healthz
curl http://127.0.0.1:9090/ready
```

### Export Metrics to CSV (for analysis)

```bash
# Fetch all metrics
curl http://127.0.0.1:9090/metrics > neo-metrics.txt

# Filter specific metrics
grep "^neo_sync" neo-metrics.txt > sync_metrics.txt
grep "^neo_peer_count\|^neo_block_height" neo-metrics.txt > basic_metrics.txt
```

### Query Metrics with PromQL (if Prometheus is running)

```bash
# Current TPS
curl "http://localhost:9090/api/v1/query?query=rate(neo_transactions_verified_total[1m])"

# Header lag
curl "http://localhost:9090/api/v1/query?query=neo_header_lag"

# Cuckoo hash hit rate
curl "http://localhost:9090/api/v1/query?query=(sum(rate(neo_cuckoo_hash_lookups_total[1h]))/(sum(rate(neo_cuckoo_hash_lookups_total[1h]))+sum(rate(neo_cuckoo_hash_lookups_miss_total[1h]))))*100"
```

---

## Example Configuration Scenarios

### Scenario 1: Development/Test Environment

Minimal setup for local testing:

```yaml
# .env
GRAFANA_ADMIN_PASSWORD=test123
SMTP_PASSWORD=test-smtp-password
SLACK_WEBHOOK_URL=http://localhost:3001/webhook  # Mock webhook

# Skip email notifications by commenting out SMTP settings
# SMTP_PASSWORD=
```

### Scenario 2: Staging Environment

Production-like with real notifications:

```yaml
# .env
GRAFANA_ADMIN_PASSWORD=Staging@Secure2026!
SMTP_PASSWORD=$(cat /run/secrets/smtp-password)
SLACK_WEBHOOK_URL=https://hooks.slack.com/services/staging-webhook-url
PAGERDUTY_SERVICE_KEY=$(cat /run/secrets/pagerduty-key)

# Configure different retention
PROMETHEUS_RETENTION_TIME=7d
```

### Scenario 3: Production Environment

Full production deployment with security:

```yaml
# .env
GRAFANA_ADMIN_PASSWORD=$(openssl rand -base64 32)
SMTP_PASSWORD=$(cat /etc/secrets/r3e-smtp-pass)
SLACK_WEBHOOK_URL=https://hooks.slack.com/services/production/webhook
PAGERDUTY_SERVICE_KEY=$(cat /etc/secrets/pagerduty-production)

# Extended retention
PROMETHEUS_RETENTION_TIME=90d

# Separate notification channels per severity
SLACK_CRITICAL_WEBHOOK_URL=https://hooks.slack.com/services/critical-webhook
SLACK_WARNING_WEBHOOK_URL=https://hooks.slack.com/services/warning-webhook
```

---

## Troubleshooting Common Issues

### Issue: "Port already in use"

**Symptoms:**
```
Error: bind: address already in use
```

**Solution:**
```bash
# Find what's using the port
netstat -ano | findstr :9090  # Windows
lsof -i :9090                 # Linux/Mac

# Kill the process or change port in config
sed -i 's/port = 9090/port = 9091/g' config/prometheus/prometheus.yml
```

### Issue: "Metrics not appearing in Prometheus"

**Symptoms:**
- Prometheus scrapes successfully but no data
- Target shows as unhealthy

**Diagnosis:**
```bash
# Check if node metrics endpoint responds
curl -v http://127.0.0.1:9090/metrics

# Check Prometheus scrape logs
docker logs prometheus | grep neo-node

# Verify firewall allows connection
iptables -L -n | grep 9090
```

**Solution:**
- Ensure neo-node is running with monitoring enabled
- Verify metrics_port is set correctly in testnet-production.toml
- Check network connectivity between containers if using Docker

### Issue: "Grafana dashboard import fails"

**Solution:**
```bash
# Check Prometheus is accessible from Grafana
curl http://localhost:9090/api/v1/status

# Try importing manually
docker exec -it grafana bash
grafana-cli plugins install grafana-piechart-panel
exit

# Re-import via web UI
# 1. Open http://localhost:3000
# 2. Dashboards > Import
# 3. Upload config/grafana/neo-node-dashboard.json
# 4. Select Prometheus as data source
```

### Issue: "Alerts not firing"

**Troubleshooting steps:**

```bash
# 1. Test PromQL query directly
curl "http://localhost:9090/api/v1/query?query=neo_header_lag"

# 2. Check alert rules loaded
curl http://localhost:9090/-/rules | jq '.'

# 3. Verify Alertmanager configuration
curl http://localhost:9093/api/v1/status

# 4. Check alert history
curl http://localhost:9093/api/v1/alerts
```

**Common causes:**
- Query returns no data (check metric names match exactly)
- For duration too long (increase evaluation interval)
- Threshold never reached (adjust alert conditions)

---

## Performance Tuning

### Optimize Prometheus Scraping

For high-throughput environments:

```yaml
# config/prometheus/prometheus.yml
global:
  scrape_interval: 5s      # More frequent scraping
  evaluation_interval: 5s
  
  # Timeout for each scrape
  scrape_timeout: 3s

# Storage optimization
storage:
  tsdb:
    retention.time: 30d
    # Compaction at lower intervals
    compaction.interval: 5m
```

### Reduce Memory Usage

```bash
# Limit Prometheus memory usage
docker run -e LIMIT_MEMORY=true prom/prometheus

# Configure garbage collection in neo-node
export GOGC=50  # Lower value = more aggressive GC
```

### Scale Horizontally

For multiple nodes:

```yaml
# monitoring-docker-compose.yml
version: '3.8'
services:
  prometheus-cluster:
    image: prom/prometheus:v2.47.0
    command:
      - '--config.file=/etc/prometheus/prometheus.yml'
      - '--storage.tsdb.path=/prometheus'
      - '--web.enable-lifecycle'
      # Cluster mode configuration
      - '--cluster.listen-address=[$(hostname -I | cut -d' ' -f1)]:9091'
  
  alertmanager-cluster:
    # Similar cluster configuration
```

---

## Security Best Practices

### Network Isolation

```yaml
# In docker-compose.yml
networks:
  monitoring-net:
    driver: bridge
    internal: true  # No external access

services:
  prometheus:
    networks:
      - monitoring-net
    ports: []  # Don't expose publicly
  
  neo-node:
    networks:
      - monitoring-net
    ports:
      - "9090:9090"  # Only expose metrics
```

### Authentication (Advanced)

For production deployments requiring authentication:

```bash
# Use reverse proxy with authentication
nginx -t
nginx -c /path/to/nginx.conf

# Nginx configuration example
location /prometheus {
    auth_basic "Restricted";
    auth_basic_user_file /etc/nginx/.htpasswd;
    proxy_pass http://localhost:9090;
}
```

Create password file:
```bash
apt-get install apache2-common  # For htpasswd
htpasswd -bc /etc/nginx/.htpasswd admin YourStrongPassword
```

---

## Maintenance Procedures

### Regular Backup

```bash
# Backup Prometheus data
tar czf /backup/prometheus-backup-$(date +%Y%m%d).tar.gz /var/lib/prometheus

# Backup Grafana dashboards
curl -u admin:password http://localhost:3000/api/search > dashboards-export.json

# Schedule weekly backup
crontab -e
# Add: 0 2 * * 0 /path/to/backup-script.sh
```

### Log Rotation

```bash
# Rotate neo-node logs
mv ./logs/neo-node-testnet.log ./logs/neo-node-testnet.log.$(date +%Y%m%d_%H%M%S)

# Clean old logs (>30 days)
find ./logs -name "*.log.*" -mtime +30 -delete

# Monitor log sizes
du -sh ./logs/*.log*
```

### Configuration Updates

```bash
# Hot reload Prometheus
curl -X POST http://localhost:9090/-/reload

# Soft reload Alertmanager
curl -X POST http://localhost:9093/api/v1/reload

# Restart Grafana after config changes
docker restart grafana
```

---

## Monitoring Checklist

### Daily
- [ ] Review Grafana dashboard for anomalies
- [ ] Check critical alerts (if any)
- [ ] Confirm sync progress (lag < 100 blocks)
- [ ] Verify peer count stable (> 25 peers)

### Weekly
- [ ] Analyze optimization effectiveness (cuckoo hit rate > 70%)
- [ ] Review system resource trends
- [ ] Check log rotation is working correctly
- [ ] Review alert performance (false positives/negatives)

### Monthly
- [ ] Review and adjust alert thresholds
- [ ] Analyze long-term performance trends
- [ ] Update baselines if needed
- [ ] Review and update runbooks
- [ ] Plan capacity based on growth

---

## Resources

- **Full Documentation:** `docs/MONITORING.md`
- **Quick Reference:** `docs/MONITORING-QUICK-REF.md`
- **Setup Summary:** `PRODUCTION_MONITORING_SETUP_SUMMARY.md`
- **Prometheus Docs:** https://prometheus.io/docs/
- **Grafana Docs:** https://grafana.com/docs/
- **Alertmanager Docs:** https://github.com/prometheus/alertmanager

---

**Need Help?** See `docs/MONITORING.md` troubleshooting section or open an issue on GitHub.
