# Neo-RS Docker Deployment Guide

**Version:** 1.0  
**Last Updated:** July 27, 2025  
**Target Audience:** DevOps, System Administrators

---

## Table of Contents

1. [Quick Start](#quick-start)
2. [Docker Setup](#docker-setup)
3. [Docker Compose Deployment](#docker-compose-deployment)
4. [Production Configuration](#production-configuration)
5. [Monitoring Stack](#monitoring-stack)
6. [Troubleshooting](#troubleshooting)
7. [Maintenance](#maintenance)

---

## Quick Start

### Prerequisites

```bash
# Install Docker and Docker Compose
sudo apt update
sudo apt install -y docker.io docker-compose

# Add user to docker group
sudo usermod -aG docker $USER
newgrp docker

# Verify installation
docker --version
docker-compose --version
```

### Immediate Deployment

```bash
# Clone repository
git clone <repository-url> neo-rs
cd neo-rs

# Create required directories
mkdir -p data logs config

# Start Neo-RS with Docker Compose
docker-compose up -d neo-rs

# Check status
docker-compose ps
docker-compose logs -f neo-rs
```

### Verify Deployment

```bash
# Test RPC endpoint
curl -X POST http://localhost:30332/rpc \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}'

# Check container health
docker inspect neo-rs-node --format='{{.State.Health.Status}}'
```

---

## Docker Setup

### Single Container Deployment

#### Build Image

```bash
# Build Neo-RS Docker image
docker build -t neo-rs:latest .

# Verify image
docker images | grep neo-rs
```

#### Run Container

```bash
# Run with default configuration (TestNet)
docker run -d \
  --name neo-rs-node \
  -p 30332:30332 \
  -p 30334:30334 \
  -v neo-rs-data:/opt/neo-rs/data \
  -v neo-rs-logs:/opt/neo-rs/logs \
  -e NEO_NETWORK=testnet \
  -e NEO_RPC_PORT=30332 \
  -e NEO_P2P_PORT=30334 \
  --restart unless-stopped \
  neo-rs:latest

# Check logs
docker logs -f neo-rs-node
```

#### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `NEO_NETWORK` | testnet | Network type (testnet/mainnet) |
| `NEO_RPC_PORT` | 30332 | RPC server port |
| `NEO_P2P_PORT` | 30334 | P2P network port |
| `NEO_DATA_PATH` | /opt/neo-rs/data | Data directory path |
| `NEO_LOG_LEVEL` | info | Logging level |
| `NEO_MAX_PEERS` | 100 | Maximum peer connections |
| `NEO_RPC_BIND` | 0.0.0.0 | RPC bind address |

---

## Docker Compose Deployment

### Basic Deployment

```yaml
# docker-compose.yml (minimal)
version: '3.8'

services:
  neo-rs:
    build: .
    image: neo-rs:latest
    container_name: neo-rs-node
    restart: unless-stopped
    ports:
      - "30332:30332"
      - "30334:30334"
    environment:
      - NEO_NETWORK=testnet
    volumes:
      - ./data:/opt/neo-rs/data
      - ./logs:/opt/neo-rs/logs
    healthcheck:
      test: ["/opt/neo-rs/bin/healthcheck.sh"]
      interval: 30s
      timeout: 10s
      retries: 3
```

### Production Deployment

```bash
# Use the full docker-compose.yml for production
docker-compose up -d neo-rs

# With monitoring (optional)
docker-compose --profile monitoring up -d

# With logging (optional)
docker-compose --profile logging up -d

# Full stack
docker-compose --profile monitoring --profile logging up -d
```

### Service Profiles

| Profile | Services | Purpose |
|---------|----------|---------|
| default | neo-rs | Core blockchain node |
| monitoring | + prometheus, grafana | Performance monitoring |
| logging | + elasticsearch, kibana, filebeat | Log management |
| cache | + redis | Caching layer |
| proxy | + nginx | Reverse proxy |

---

## Production Configuration

### Directory Structure

```bash
# Create production directory structure
mkdir -p neo-rs-production/{data,logs,config,monitoring,backups}
cd neo-rs-production

# Copy configuration files
cp /path/to/neo-rs/docker-compose.yml .
cp -r /path/to/neo-rs/docker ./
cp -r /path/to/neo-rs/monitoring ./
```

### Configuration Files

#### Environment Configuration

```bash
# Create .env file for production
cat > .env << 'EOF'
# Neo-RS Production Configuration
NEO_NETWORK=testnet
NEO_RPC_PORT=30332
NEO_P2P_PORT=30334
NEO_LOG_LEVEL=info
NEO_MAX_PEERS=100

# Resource limits
MEMORY_LIMIT=1g
CPU_LIMIT=1.0

# Volumes
DATA_PATH=./data
LOGS_PATH=./logs
CONFIG_PATH=./config

# Monitoring
PROMETHEUS_PORT=9090
GRAFANA_PORT=3000
GRAFANA_PASSWORD=secure_password_here

# Security
CONTAINER_USER=1000:1000
EOF
```

#### Node Configuration

```bash
# Create node configuration
mkdir -p config
cat > config/neo-rs.toml << 'EOF'
[network]
network = "testnet"
rpc_port = 30332
p2p_port = 30334
max_peers = 100

[logging]
level = "info"
file = "/opt/neo-rs/logs/neo-node.log"

[storage]
data_path = "/opt/neo-rs/data"
cache_size = "512MB"

[rpc]
bind_address = "0.0.0.0"
cors_enabled = false
max_connections = 100
EOF
```

### Production Docker Compose

```yaml
# docker-compose.prod.yml
version: '3.8'

services:
  neo-rs:
    build:
      context: .
      dockerfile: Dockerfile
      args:
        - BUILD_ENV=production
    image: neo-rs:production
    container_name: neo-rs-prod
    restart: unless-stopped
    
    ports:
      - "30332:30332"
      - "30334:30334"
    
    environment:
      - NEO_NETWORK=${NEO_NETWORK}
      - NEO_RPC_PORT=${NEO_RPC_PORT}
      - NEO_P2P_PORT=${NEO_P2P_PORT}
      - NEO_LOG_LEVEL=${NEO_LOG_LEVEL}
      - NEO_MAX_PEERS=${NEO_MAX_PEERS}
    
    volumes:
      - ${DATA_PATH}:/opt/neo-rs/data
      - ${LOGS_PATH}:/opt/neo-rs/logs
      - ${CONFIG_PATH}:/opt/neo-rs/config:ro
    
    deploy:
      resources:
        limits:
          memory: ${MEMORY_LIMIT}
          cpus: '${CPU_LIMIT}'
        reservations:
          memory: 256M
          cpus: '0.25'
    
    healthcheck:
      test: ["/opt/neo-rs/bin/healthcheck.sh"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 60s
    
    user: ${CONTAINER_USER}
    security_opt:
      - no-new-privileges:true
      - apparmor:unconfined
    
    logging:
      driver: "json-file"
      options:
        max-size: "100m"
        max-file: "5"
        labels: "service=neo-rs"
    
    networks:
      - neo-network

  # Reverse proxy for security
  nginx:
    image: nginx:alpine
    container_name: neo-rs-proxy
    restart: unless-stopped
    ports:
      - "80:80"
      - "443:443"
    volumes:
      - ./nginx/nginx.conf:/etc/nginx/nginx.conf:ro
      - ./nginx/ssl:/etc/nginx/ssl:ro
    depends_on:
      - neo-rs
    networks:
      - neo-network

networks:
  neo-network:
    driver: bridge
    ipam:
      config:
        - subnet: 172.20.0.0/16
```

### Security Configuration

#### Nginx Reverse Proxy

```nginx
# nginx/nginx.conf
upstream neo-rs-backend {
    server neo-rs:30332;
}

server {
    listen 80;
    server_name neo-rpc.yourdomain.com;
    return 301 https://$server_name$request_uri;
}

server {
    listen 443 ssl http2;
    server_name neo-rpc.yourdomain.com;
    
    ssl_certificate /etc/nginx/ssl/cert.pem;
    ssl_certificate_key /etc/nginx/ssl/key.pem;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers ECDHE-RSA-AES256-GCM-SHA512:DHE-RSA-AES256-GCM-SHA512;
    
    # Security headers
    add_header X-Frame-Options DENY;
    add_header X-Content-Type-Options nosniff;
    add_header X-XSS-Protection "1; mode=block";
    add_header Strict-Transport-Security "max-age=63072000";
    
    # Rate limiting
    limit_req_zone $binary_remote_addr zone=api:10m rate=10r/s;
    
    location /rpc {
        limit_req zone=api burst=20 nodelay;
        
        proxy_pass http://neo-rs-backend/rpc;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        
        # Timeouts
        proxy_connect_timeout 10s;
        proxy_send_timeout 30s;
        proxy_read_timeout 30s;
    }
    
    location /health {
        access_log off;
        return 200 "healthy\n";
        add_header Content-Type text/plain;
    }
}
```

---

## Monitoring Stack

### Enable Monitoring

```bash
# Start with monitoring stack
docker-compose --profile monitoring up -d

# Access Grafana
open http://localhost:3000
# Login: admin / admin (change on first login)

# Access Prometheus
open http://localhost:9090
```

### Monitoring Configuration

#### Prometheus Configuration

```yaml
# monitoring/prometheus.yml
global:
  scrape_interval: 15s
  evaluation_interval: 15s

rule_files:
  - "alerts.yml"

scrape_configs:
  - job_name: 'neo-rs'
    static_configs:
      - targets: ['neo-rs:30333']  # Metrics endpoint
    scrape_interval: 10s
    
  - job_name: 'docker'
    static_configs:
      - targets: ['host.docker.internal:9323']
    scrape_interval: 15s

alerting:
  alertmanagers:
    - static_configs:
        - targets:
          - alertmanager:9093
```

#### Grafana Dashboard

```json
# monitoring/grafana/dashboards/neo-rs.json
{
  "dashboard": {
    "title": "Neo-RS Production Dashboard",
    "panels": [
      {
        "title": "Service Status",
        "type": "stat",
        "targets": [{
          "expr": "neo_rs_up"
        }]
      },
      {
        "title": "RPC Response Time",
        "type": "graph",
        "targets": [{
          "expr": "neo_rs_rpc_response_time_ms"
        }]
      },
      {
        "title": "Memory Usage",
        "type": "graph", 
        "targets": [{
          "expr": "neo_rs_memory_bytes / 1024 / 1024"
        }]
      }
    ]
  }
}
```

---

## Troubleshooting

### Common Docker Issues

#### Container Won't Start

```bash
# Check container status
docker-compose ps

# View logs
docker-compose logs neo-rs

# Check system resources
docker system df
docker system events

# Inspect container
docker inspect neo-rs-node
```

#### Port Binding Issues

```bash
# Check port usage
sudo netstat -tulpn | grep -E ":30332|:30334"

# Kill conflicting processes
sudo pkill -f neo-node

# Use different ports
docker-compose down
# Edit docker-compose.yml to use different ports
docker-compose up -d
```

#### Volume Mount Issues

```bash
# Check volume permissions
ls -la data/ logs/

# Fix permissions
sudo chown -R 1000:1000 data/ logs/

# Check volume mounts
docker inspect neo-rs-node | grep -A 10 "Mounts"
```

#### Memory Issues

```bash
# Check container memory usage
docker stats neo-rs-node

# Check system memory
free -h

# Increase container memory limit
# Edit docker-compose.yml and increase memory limit
docker-compose up -d --force-recreate
```

### Health Check Debugging

```bash
# Manual health check
docker exec neo-rs-node /opt/neo-rs/bin/healthcheck.sh

# Check health status
docker inspect neo-rs-node --format='{{.State.Health.Status}}'

# View health check logs
docker inspect neo-rs-node --format='{{range .State.Health.Log}}{{.Output}}{{end}}'
```

### Network Debugging

```bash
# Check container networks
docker network ls
docker network inspect neo-rs_neo-network

# Test internal connectivity
docker exec neo-rs-node curl http://localhost:30332/rpc

# Test external connectivity  
curl http://localhost:30332/rpc
```

---

## Maintenance

### Regular Maintenance Tasks

#### Daily

```bash
# Check container health
docker-compose ps
docker inspect neo-rs-node --format='{{.State.Health.Status}}'

# Monitor logs
docker-compose logs --tail=100 neo-rs

# Check resource usage
docker stats --no-stream neo-rs-node
```

#### Weekly

```bash
# Backup data
docker run --rm \
  -v neo-rs_neo-rs-data:/source:ro \
  -v $(pwd)/backups:/backup \
  ubuntu tar czf /backup/neo-rs-data-$(date +%Y%m%d).tar.gz -C /source .

# Clean up old logs
docker exec neo-rs-node find /opt/neo-rs/logs -name "*.log" -mtime +7 -delete

# Update images (if needed)
docker-compose pull
docker-compose up -d --force-recreate
```

#### Monthly

```bash
# System cleanup
docker system prune -f
docker volume prune -f

# Security updates
docker pull neo-rs:latest
docker-compose up -d --force-recreate

# Performance review
docker stats --no-stream
df -h
```

### Backup and Recovery

#### Data Backup

```bash
# Create backup script
cat > backup-neo-rs.sh << 'EOF'
#!/bin/bash
BACKUP_DIR="./backups"
DATE=$(date +%Y%m%d_%H%M%S)

mkdir -p "$BACKUP_DIR"

# Stop containers
docker-compose stop neo-rs

# Backup data volume
docker run --rm \
  -v neo-rs_neo-rs-data:/source:ro \
  -v $(pwd)/backups:/backup \
  ubuntu tar czf /backup/neo-rs-data-${DATE}.tar.gz -C /source .

# Backup logs volume
docker run --rm \
  -v neo-rs_neo-rs-logs:/source:ro \
  -v $(pwd)/backups:/backup \
  ubuntu tar czf /backup/neo-rs-logs-${DATE}.tar.gz -C /source .

# Start containers
docker-compose start neo-rs

echo "Backup completed: $DATE"
EOF

chmod +x backup-neo-rs.sh
```

#### Recovery Procedure

```bash
# Stop services
docker-compose down

# Remove old volumes (BE CAREFUL!)
docker volume rm neo-rs_neo-rs-data neo-rs_neo-rs-logs

# Restore from backup
BACKUP_FILE="backups/neo-rs-data-20250727_120000.tar.gz"

docker run --rm \
  -v neo-rs_neo-rs-data:/target \
  -v $(pwd)/backups:/backup \
  ubuntu tar xzf /backup/$(basename $BACKUP_FILE) -C /target

# Start services
docker-compose up -d
```

### Updates and Upgrades

```bash
# Update Docker images
docker-compose pull

# Recreate containers with new images
docker-compose up -d --force-recreate

# Verify update
docker images | grep neo-rs
docker-compose logs neo-rs
```

---

## Deployment Checklist

### Pre-Deployment

- [ ] Docker and Docker Compose installed
- [ ] Required directories created (data, logs, config)
- [ ] Configuration files prepared
- [ ] SSL certificates ready (if using HTTPS)
- [ ] Firewall rules configured
- [ ] Backup strategy planned

### Deployment

- [ ] Images built successfully
- [ ] Containers started without errors
- [ ] Health checks passing
- [ ] RPC endpoint responding
- [ ] Monitoring configured (if enabled)
- [ ] Log aggregation working (if enabled)

### Post-Deployment

- [ ] Performance baseline established
- [ ] Alerting configured
- [ ] Backup schedule configured
- [ ] Documentation updated
- [ ] Team trained on Docker operations

---

**Related Documentation:**
- [Deployment Guide](DEPLOYMENT_GUIDE.md)
- [Monitoring Guide](MONITORING_GUIDE.md)
- [Troubleshooting Guide](TROUBLESHOOTING_GUIDE.md)