# Neo-RS Deployment Guide

**Version:** 1.0  
**Last Updated:** July 27, 2025  
**Compatibility:** Neo-RS TestNet/MainNet

---

## Table of Contents

1. [Prerequisites](#prerequisites)
2. [Quick Start Deployment](#quick-start-deployment)
3. [Production Deployment](#production-deployment)
4. [Configuration Management](#configuration-management)
5. [Monitoring & Health Checks](#monitoring--health-checks)
6. [Security Considerations](#security-considerations)
7. [Troubleshooting](#troubleshooting)
8. [Backup & Recovery](#backup--recovery)

---

## Prerequisites

### System Requirements

**Minimum Requirements:**
- OS: Linux (Ubuntu 20.04+), macOS (10.15+), Windows 10+
- RAM: 512MB (recommended: 2GB+)
- Storage: 10GB available space
- Network: Outbound internet access on ports 20333, 30332, 30334

**Recommended Production Requirements:**
- RAM: 4GB+
- Storage: 50GB+ SSD
- CPU: 2+ cores
- Network: Stable broadband connection

### Dependencies

```bash
# Ubuntu/Debian
sudo apt update && sudo apt install -y \
    build-essential \
    curl \
    git \
    pkg-config \
    libssl-dev \
    lsof \
    bc

# macOS (via Homebrew)
brew install curl git lsof bc

# Rust (all platforms)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
```

### Build Requirements

```bash
# Clone and build Neo-RS
git clone <repository-url> neo-rs
cd neo-rs
cargo build --release

# Verify build
./target/release/neo-node --version
```

---

## Quick Start Deployment

### 1. Basic Setup (Development)

```bash
# Navigate to project directory
cd neo-rs

# Quick start for development
./start-node-safe.sh

# Verify deployment
curl -X POST http://localhost:30332/rpc \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}'
```

### 2. Environment Configuration

```bash
# Set environment variables
export NEO_NETWORK=testnet
export NEO_RPC_PORT=30332
export NEO_P2P_PORT=30334
export NEO_DATA_PATH=$HOME/.neo-rs-production
```

---

## Production Deployment

### 1. Production Environment Setup

#### Create Production User
```bash
# Create dedicated user (Linux)
sudo useradd -r -m -s /bin/bash neo-rs
sudo usermod -aG docker neo-rs  # Optional: for Docker deployment

# Switch to production user
sudo -u neo-rs -i
```

#### Directory Structure
```bash
# Create production directory structure
mkdir -p /opt/neo-rs/{bin,config,data,logs,scripts,backups}
chown -R neo-rs:neo-rs /opt/neo-rs

# Production layout:
/opt/neo-rs/
├── bin/                 # Neo-RS binaries
├── config/             # Configuration files
├── data/               # Blockchain data
├── logs/               # Application logs
├── scripts/            # Management scripts
└── backups/            # Data backups
```

#### Copy Production Files
```bash
# Copy binaries
cp target/release/neo-node /opt/neo-rs/bin/
cp start-node-safe.sh /opt/neo-rs/scripts/
cp production-readiness-assessment.sh /opt/neo-rs/scripts/

# Set permissions
chmod +x /opt/neo-rs/bin/neo-node
chmod +x /opt/neo-rs/scripts/*.sh
```

### 2. Production Configuration

#### Create Production Config
```bash
# Create production configuration
cat > /opt/neo-rs/config/production.env << 'EOF'
# Neo-RS Production Configuration
NEO_NETWORK=testnet
NEO_RPC_PORT=30332
NEO_P2P_PORT=30334
NEO_DATA_PATH=/opt/neo-rs/data
NEO_LOG_LEVEL=info
NEO_MAX_PEERS=100
NEO_ENABLE_CORS=false
NEO_RPC_BIND=127.0.0.1
EOF
```

#### Create Systemd Service
```bash
# Create systemd service file
sudo tee /etc/systemd/system/neo-rs.service << 'EOF'
[Unit]
Description=Neo-RS Blockchain Node
After=network.target
Wants=network-online.target

[Service]
Type=simple
User=neo-rs
Group=neo-rs
WorkingDirectory=/opt/neo-rs
ExecStart=/opt/neo-rs/bin/neo-node --testnet --data-path /opt/neo-rs/data --rpc-port 30332 --p2p-port 30334
ExecReload=/bin/kill -HUP $MAINPID
KillMode=process
Restart=always
RestartSec=10
StandardOutput=journal
StandardError=journal
SyslogIdentifier=neo-rs

# Security settings
NoNewPrivileges=yes
PrivateTmp=yes
ProtectSystem=strict
ReadWritePaths=/opt/neo-rs/data /opt/neo-rs/logs
ProtectHome=yes

# Resource limits
LimitNOFILE=65536
LimitNPROC=32768

[Install]
WantedBy=multi-user.target
EOF

# Enable and start service
sudo systemctl daemon-reload
sudo systemctl enable neo-rs
sudo systemctl start neo-rs
```

### 3. Production Scripts

#### Health Check Script
```bash
# Create health check script
cat > /opt/neo-rs/scripts/health-check.sh << 'EOF'
#!/bin/bash
set -e

RPC_PORT=30332
TIMEOUT=10

# Check if process is running
if ! pgrep -f neo-node > /dev/null; then
    echo "CRITICAL: Neo-RS process not running"
    exit 2
fi

# Check RPC responsiveness
if ! curl -s --connect-timeout $TIMEOUT --max-time $TIMEOUT \
    -X POST http://localhost:$RPC_PORT/rpc \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}' | \
    grep -q "result"; then
    echo "CRITICAL: RPC endpoint not responding"
    exit 2
fi

echo "OK: Neo-RS healthy"
exit 0
EOF

chmod +x /opt/neo-rs/scripts/health-check.sh
```

#### Log Rotation
```bash
# Create logrotate configuration
sudo tee /etc/logrotate.d/neo-rs << 'EOF'
/opt/neo-rs/logs/*.log {
    daily
    rotate 30
    compress
    delaycompress
    missingok
    notifempty
    postrotate
        systemctl reload neo-rs || true
    endscript
}
EOF
```

---

## Configuration Management

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `NEO_NETWORK` | testnet | Network type (testnet/mainnet) |
| `NEO_RPC_PORT` | 30332 | RPC server port |
| `NEO_P2P_PORT` | 30334 | P2P network port |
| `NEO_DATA_PATH` | ~/.neo-rs | Blockchain data directory |
| `NEO_LOG_LEVEL` | info | Logging level (debug/info/warn/error) |
| `NEO_MAX_PEERS` | 100 | Maximum peer connections |
| `NEO_RPC_BIND` | 0.0.0.0 | RPC bind address |

### Network Configuration

#### Firewall Setup
```bash
# Ubuntu/Debian (ufw)
sudo ufw allow 30332/tcp  # RPC port
sudo ufw allow 30334/tcp  # P2P port
sudo ufw enable

# CentOS/RHEL (firewalld)
sudo firewall-cmd --permanent --add-port=30332/tcp
sudo firewall-cmd --permanent --add-port=30334/tcp
sudo firewall-cmd --reload
```

#### Reverse Proxy (Nginx)
```nginx
# /etc/nginx/sites-available/neo-rs
server {
    listen 443 ssl http2;
    server_name neo-rpc.yourdomain.com;
    
    ssl_certificate /path/to/cert.pem;
    ssl_certificate_key /path/to/key.pem;
    
    location /rpc {
        proxy_pass http://127.0.0.1:30332/rpc;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        
        # Security headers
        add_header X-Frame-Options DENY;
        add_header X-Content-Type-Options nosniff;
        add_header X-XSS-Protection "1; mode=block";
    }
    
    # Health check endpoint
    location /health {
        access_log off;
        return 200 "healthy\n";
        add_header Content-Type text/plain;
    }
}
```

---

## Monitoring & Health Checks

### Prometheus Metrics

```bash
# Create metrics exposure script
cat > /opt/neo-rs/scripts/metrics.sh << 'EOF'
#!/bin/bash

# Neo-RS metrics for Prometheus
echo "# HELP neo_rs_up Node availability"
echo "# TYPE neo_rs_up gauge"
if pgrep -f neo-node > /dev/null; then
    echo "neo_rs_up 1"
else
    echo "neo_rs_up 0"
fi

# Memory usage
MEMORY=$(ps -o rss= -p $(pgrep neo-node) 2>/dev/null | tr -d ' ' || echo "0")
echo "# HELP neo_rs_memory_bytes Memory usage in bytes"
echo "# TYPE neo_rs_memory_bytes gauge"
echo "neo_rs_memory_bytes $((MEMORY * 1024))"

# RPC response time
START=$(date +%s%N)
curl -s -X POST http://localhost:30332/rpc \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}' > /dev/null 2>&1
END=$(date +%s%N)
RESPONSE_TIME=$(( (END - START) / 1000000 ))

echo "# HELP neo_rs_rpc_response_time_ms RPC response time in milliseconds"
echo "# TYPE neo_rs_rpc_response_time_ms gauge"
echo "neo_rs_rpc_response_time_ms $RESPONSE_TIME"
EOF

chmod +x /opt/neo-rs/scripts/metrics.sh
```

### Alerting Rules

```yaml
# prometheus-alerts.yml
groups:
- name: neo-rs
  rules:
  - alert: NeoRSDown
    expr: neo_rs_up == 0
    for: 1m
    labels:
      severity: critical
    annotations:
      summary: "Neo-RS node is down"
      description: "Neo-RS node has been down for more than 1 minute"

  - alert: NeoRSHighMemory
    expr: neo_rs_memory_bytes > 100 * 1024 * 1024
    for: 5m
    labels:
      severity: warning
    annotations:
      summary: "Neo-RS high memory usage"
      description: "Neo-RS memory usage is above 100MB"

  - alert: NeoRSSlowResponse
    expr: neo_rs_rpc_response_time_ms > 1000
    for: 2m
    labels:
      severity: warning
    annotations:
      summary: "Neo-RS slow RPC responses"
      description: "RPC response time is above 1 second"
```

---

## Security Considerations

### Network Security

1. **Firewall Configuration**
   - Only expose required ports (30332 for RPC, 30334 for P2P)
   - Use allowlist for RPC access in production
   - Consider VPN access for administrative functions

2. **RPC Security**
   - Bind RPC to localhost (127.0.0.1) by default
   - Use reverse proxy with SSL/TLS termination
   - Implement rate limiting
   - Add authentication if required

3. **System Security**
   - Run as dedicated non-root user
   - Use systemd security features
   - Regular security updates
   - File system permissions

### SSL/TLS Configuration

```bash
# Generate self-signed certificate for testing
openssl req -x509 -newkey rsa:4096 -keyout /opt/neo-rs/config/key.pem \
  -out /opt/neo-rs/config/cert.pem -days 365 -nodes \
  -subj "/C=US/ST=State/L=City/O=Organization/CN=neo-rpc.local"
```

---

## Troubleshooting

### Common Issues

#### 1. Port Already in Use
```bash
# Check what's using the port
sudo lsof -i :30332
sudo lsof -i :30334

# Kill conflicting process
sudo pkill -f neo-node
```

#### 2. Permission Denied
```bash
# Fix ownership
sudo chown -R neo-rs:neo-rs /opt/neo-rs

# Fix permissions
sudo chmod -R 755 /opt/neo-rs
sudo chmod -R 644 /opt/neo-rs/config/*
```

#### 3. Memory Issues
```bash
# Check system memory
free -h

# Monitor neo-rs memory usage
watch 'ps aux | grep neo-node'
```

#### 4. Network Connectivity
```bash
# Test RPC connectivity
curl -v -X POST http://localhost:30332/rpc \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}'

# Test P2P connectivity
telnet seed-node-ip 20333
```

### Log Analysis

```bash
# View real-time logs
sudo journalctl -u neo-rs -f

# Search for errors
sudo journalctl -u neo-rs | grep -i error

# View logs from last hour
sudo journalctl -u neo-rs --since "1 hour ago"
```

---

## Backup & Recovery

### Data Backup

```bash
# Create backup script
cat > /opt/neo-rs/scripts/backup.sh << 'EOF'
#!/bin/bash
set -e

BACKUP_DIR="/opt/neo-rs/backups"
DATE=$(date +%Y%m%d_%H%M%S)
DATA_DIR="/opt/neo-rs/data"

# Stop service
sudo systemctl stop neo-rs

# Create backup
tar -czf "$BACKUP_DIR/neo-rs-backup-$DATE.tar.gz" -C "$DATA_DIR" .

# Start service
sudo systemctl start neo-rs

# Cleanup old backups (keep 7 days)
find "$BACKUP_DIR" -name "neo-rs-backup-*.tar.gz" -mtime +7 -delete

echo "Backup completed: neo-rs-backup-$DATE.tar.gz"
EOF

chmod +x /opt/neo-rs/scripts/backup.sh

# Schedule daily backups
echo "0 2 * * * /opt/neo-rs/scripts/backup.sh" | sudo crontab -u neo-rs -
```

### Recovery Procedure

```bash
# Stop service
sudo systemctl stop neo-rs

# Restore from backup
cd /opt/neo-rs/data
sudo rm -rf *
sudo tar -xzf /opt/neo-rs/backups/neo-rs-backup-YYYYMMDD_HHMMSS.tar.gz

# Fix permissions
sudo chown -R neo-rs:neo-rs /opt/neo-rs/data

# Start service
sudo systemctl start neo-rs
```

---

## Performance Tuning

### System Optimization

```bash
# Increase file descriptor limits
echo "neo-rs soft nofile 65536" | sudo tee -a /etc/security/limits.conf
echo "neo-rs hard nofile 65536" | sudo tee -a /etc/security/limits.conf

# Optimize network settings
echo "net.core.rmem_max = 16777216" | sudo tee -a /etc/sysctl.conf
echo "net.core.wmem_max = 16777216" | sudo tee -a /etc/sysctl.conf
sudo sysctl -p
```

### Database Optimization

```bash
# RocksDB tuning environment variables
export ROCKSDB_CACHE_SIZE=512MB
export ROCKSDB_WRITE_BUFFER_SIZE=64MB
export ROCKSDB_MAX_OPEN_FILES=1000
```

---

## Deployment Checklist

### Pre-Deployment

- [ ] System requirements verified
- [ ] Dependencies installed
- [ ] Neo-RS binary built and tested
- [ ] Network ports configured
- [ ] SSL certificates prepared (if required)
- [ ] Monitoring setup configured
- [ ] Backup procedures tested

### Deployment

- [ ] Production user created
- [ ] Directory structure created
- [ ] Configuration files deployed
- [ ] Systemd service configured
- [ ] Firewall rules applied
- [ ] Service started and enabled
- [ ] Health checks passing

### Post-Deployment

- [ ] Production readiness assessment run
- [ ] Monitoring alerts configured
- [ ] Backup schedule configured
- [ ] Documentation updated
- [ ] Team trained on operations
- [ ] Incident response procedures documented

---

**Next:** [Operational Runbooks](OPERATIONAL_RUNBOOKS.md)