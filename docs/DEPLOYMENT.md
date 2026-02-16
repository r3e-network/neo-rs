# Neo-rs Deployment Guide

> **Version**: 0.7.0  
> **Last Updated**: 2026-01-28  
> **Target Compatibility**: Neo N3 v3.9.2

Comprehensive deployment documentation for the Neo N3 Rust node implementation.

## Table of Contents

- [Prerequisites](#prerequisites)
- [Build Instructions](#build-instructions)
- [Configuration](#configuration)
- [Docker Deployment](#docker-deployment)
- [Running a Node](#running-a-node)
- [Hardware Requirements](#hardware-requirements)
- [Upgrading](#upgrading)

---

## Prerequisites

### System Requirements

#### Supported Operating Systems

| OS | Version | Status |
|----|---------|--------|
| Ubuntu | 20.04 LTS, 22.04 LTS, 24.04 LTS | ✅ Fully supported |
| Debian | 11 (Bullseye), 12 (Bookworm) | ✅ Fully supported |
| CentOS/RHEL | 8, 9 | ✅ Supported |
| Alpine Linux | 3.18+ | ⚠️ Requires static linking |
| macOS | 13+ (Ventura) | ✅ Development only |
| Windows | 10/11, Server 2019+ | ⚠️ Community support |

### Dependencies

#### Required System Packages

**Ubuntu/Debian:**
```bash
sudo apt-get update
sudo apt-get install -y \
    build-essential \
    gcc \
    g++ \
    cmake \
    make \
    pkg-config \
    librocksdb-dev \
    libssl-dev \
    clang \
    git \
    curl
```

**CentOS/RHEL:**
```bash
sudo yum install -y \
    gcc \
    gcc-c++ \
    cmake \
    make \
    pkgconfig \
    openssl-devel \
    clang \
    git \
    curl

# Install RocksDB from source or EPEL
sudo yum install -y epel-release
sudo yum install -y rocksdb-devel
```

#### Rust Toolchain

Minimum supported Rust version (MSRV): **1.85.0**

```bash
# Install Rust via rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# Verify installation
rustc --version  # Should be >= 1.85.0
```

#### Optional Dependencies

| Package | Purpose | Installation |
|---------|---------|--------------|
| `docker` | Container deployment | [Docker Docs](https://docs.docker.com/engine/install/) |
| `docker-compose` | Multi-container orchestration | Included with Docker Desktop |
| `systemd` | Service management | Pre-installed on most Linux distros |
| `prometheus` | Metrics collection | See [MONITORING.md](./MONITORING.md) |

---

## Build Instructions

### Release Build

For production deployments, always use the release profile:

```bash
# Clone the repository
git clone https://github.com/r3e-network/neo-rs.git
cd neo-rs

# Build all workspace crates in release mode
cargo build --release --workspace

# Binaries will be available at:
# - target/release/neo-node  (node daemon)
# - target/release/neo-cli   (CLI client)
```

### Production Profile

For maximum performance, use the custom production profile:

```bash
# Build with production optimizations
cargo build --profile production -p neo-node -p neo-cli

# Binaries will be at:
# - target/production/neo-node
# - target/production/neo-cli
```

The production profile enables:
- LTO (Link Time Optimization) with `fat` mode
- Single codegen unit for maximum optimization
- Panic abort strategy
- Binary stripping for smaller size

### Feature Flags

Optional features for specialized deployments:

| Feature | Description | Build Command |
|---------|-------------|---------------|
| `tee` | Trusted Execution Environment support | `--features tee` |
| `tee-sgx` | TEE with Intel SGX hardware | `--features tee-sgx` |
| `hsm` | Hardware Security Module support | `--features hsm` |
| `hsm-ledger` | HSM with Ledger hardware wallet | `--features hsm-ledger` |
| `hsm-pkcs11` | HSM with PKCS#11 interface | `--features hsm-pkcs11` |

Example with TEE support:
```bash
cargo build --release -p neo-node --features tee-sgx
```

### Build Verification

```bash
# Verify binary versions
./target/release/neo-node --version
./target/release/neo-cli --version

# Run preflight checks
make preflight
```

---

## Configuration

### Config File Format

Neo-rs uses TOML configuration files. Three bundled configs are provided:

| Config File | Network | Purpose |
|-------------|---------|---------|
| `neo_mainnet_node.toml` | MainNet | Standard mainnet configuration |
| `neo_testnet_node.toml` | TestNet | Development and testing |
| `neo_production_node.toml` | MainNet | Hardened production settings |

#### Configuration Sections

```toml
# Network identity
[network]
network_magic = 0x334F454E  # MainNet: 0x334F454E, TestNet: 0x3554334E
address_version = 0x35

# Storage backend
[storage]
backend = "rocksdb"         # Options: rocksdb, memory
data_dir = "./data/mainnet"
read_only = false

# P2P networking
[p2p]
port = 10333
max_connections = 100
min_desired_connections = 10
seed_nodes = [
    "seed1.neo.org:10333",
    "seed2.neo.org:10333",
    "seed3.neo.org:10333",
    "seed4.neo.org:10333",
    "seed5.neo.org:10333"
]
enable_compression = true
broadcast_history_limit = 100000

# JSON-RPC server
[rpc]
enabled = true
port = 10332
bind_address = "127.0.0.1"
cors_enabled = false
auth_enabled = true
max_gas_invoke = 50000000
max_iterator_results = 100
disabled_methods = ["openwallet"]

# Consensus (dBFT)
[consensus]
enabled = false
auto_start = false

# Telemetry and metrics
[telemetry]
[telemetry.metrics]
enabled = false
port = 9090
bind_address = "127.0.0.1"

# Logging configuration
[logging]
level = "info"              # Options: trace, debug, info, warn, error
format = "json"             # Options: json, pretty, compact
file_path = "./logs/neo-node-mainnet.log"
max_file_size = "100MB"
max_files = 10

# Blockchain parameters
[blockchain]
block_time = 15000          # 15 seconds in milliseconds
max_transactions_per_block = 512
max_free_transactions_per_block = 20

# Memory pool
[mempool]
max_transactions = 50000
```

### Environment Variables

All configuration options can be overridden via environment variables:

| Variable | Description | Example |
|----------|-------------|---------|
| `NEO_CONFIG` | Path to TOML config file | `/etc/neo/config.toml` |
| `NEO_NETWORK` | Network selection | `mainnet`, `testnet` |
| `NEO_STORAGE` | Data directory path | `/var/neo/data` |
| `NEO_BACKEND` | Storage backend | `rocksdb`, `memory` |
| `NEO_PLUGINS_DIR` | Plugin configuration directory | `/var/neo/Plugins` |
| `NEO_NETWORK_MAGIC` | Override network magic | `860833102` |
| `NEO_LISTEN_PORT` | P2P listen port | `10333` |
| `NEO_RPC_PORT` | RPC server port | `10332` |
| `NEO_RPC_BIND` | RPC bind address | `127.0.0.1` |
| `NEO_RPC_USER` | RPC basic auth username | `admin` |
| `NEO_RPC_PASS` | RPC basic auth password | `secret` |
| `NEO_RPC_TLS_CERT` | Path to TLS certificate | `/etc/neo/cert.pem` |
| `NEO_RPC_TLS_PASS` | TLS certificate password | - |
| `NEO_MAX_CONNECTIONS` | Maximum P2P connections | `100` |
| `NEO_MIN_CONNECTIONS` | Minimum P2P connections | `10` |
| `NEO_BLOCK_TIME` | Block time in milliseconds | `15000` |
| `NEO_LOG_LEVEL` | Log level | `info` |
| `NEO_LOG_PATH` | Log file path | `/var/log/neo/node.log` |
| `NEO_HEALTH_PORT` | Health check endpoint port | `8080` |
| `NEO_HEALTH_MAX_HEADER_LAG` | Max header lag for healthy status | `20` |
| `NEO_STATE_ROOT` | Enable state root calculation | `1` |
| `NEO_STATE_ROOT_PATH` | State root DB path | `/var/neo/stateroot` |
| `RUST_LOG` | Rust logging directive | `info,neo_p2p=debug` |

### Network Selection (MainNet/TestNet)

#### Using Configuration Files

```bash
# MainNet node
./target/release/neo-node --config neo_mainnet_node.toml

# TestNet node
./target/release/neo-node --config neo_testnet_node.toml
```

#### Using Environment Variables

```bash
# The NEO_NETWORK variable auto-selects bundled configs
NEO_NETWORK=mainnet ./target/release/neo-node
NEO_NETWORK=testnet ./target/release/neo-node
```

#### Using CLI Flags

```bash
# Override specific settings
./target/release/neo-node \
    --config neo_mainnet_node.toml \
    --network-magic 860833102 \
    --listen-port 10333
```

### Configuration Validation

Validate configuration without starting the node:

```bash
# Check config syntax and paths
./target/release/neo-node --config neo_mainnet_node.toml --check-config

# Check storage connectivity
./target/release/neo-node --config neo_mainnet_node.toml --check-storage

# Run all checks
./target/release/neo-node --config neo_mainnet_node.toml --check-all

# Or use make targets
make check-config CONFIG=neo_mainnet_node.toml
make preflight  # Checks both mainnet and testnet configs
```

---

## Docker Deployment

### Docker Build

```bash
# Build the Docker image
docker build -t neo-rs:latest .

# Build with specific tag
docker build -t neo-rs:v0.7.0 .
```

### Basic Docker Run

```bash
# Run on TestNet with persistent data
docker run -d \
    --name neo-node \
    -p 20332:20332 \
    -p 20333:20333 \
    -v $(pwd)/data:/data \
    -e NEO_NETWORK=testnet \
    neo-rs:latest

# Run on MainNet
docker run -d \
    --name neo-node \
    -p 10332:10332 \
    -p 10333:10333 \
    -v $(pwd)/data:/data \
    -e NEO_NETWORK=mainnet \
    neo-rs:latest
```

### Docker Compose Setup

The project includes a `docker-compose.yml` for easy deployment:

```bash
# Copy environment template
cp .env.example .env

# Edit configuration
nano .env

# Start the node
docker compose up -d neo-node

# View logs
docker compose logs -f neo-node

# Stop the node
docker compose down
```

#### Environment Variables (.env)

```bash
# Network selection: mainnet or testnet
NEO_NETWORK=testnet

# Storage backend
NEO_BACKEND=rocksdb

# Plugin directory
NEO_PLUGINS_DIR=/data/Plugins

# Custom config (optional)
# NEO_CONFIG=/config/custom.toml

# Custom storage path (optional)
# NEO_STORAGE=/data/blockchain

# Port overrides (optional)
# NEO_RPC_PORT=20332
# NEO_LISTEN_PORT=20333

# Logging
RUST_LOG=info

# Grafana password (for monitoring profile)
GRAFANA_PASSWORD=admin
```

### Volume Mounts

Recommended directory structure for Docker volumes:

```
/data
├── mainnet/          # MainNet blockchain data
├── testnet/          # TestNet blockchain data
├── Plugins/          # Plugin configurations
│   └── RpcServer/
│       └── RpcServer.json
└── Logs/             # Log files
    └── neo-node.log
```

#### Docker Volume Configuration

```yaml
# docker-compose.yml snippet
volumes:
  # Named volume for data persistence
  neo-data:
    driver: local

  # Bind mount for custom configuration
  - ./config:/config:ro

  # Bind mount for logs on host
  - ./logs:/data/Logs
```

### Container Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `NEO_NETWORK` | `testnet` | Network selection |
| `NEO_CONFIG` | - | Custom config path |
| `NEO_STORAGE` | `/data/{network}` | Data directory |
| `NEO_BACKEND` | `rocksdb` | Storage backend |
| `NEO_PLUGINS_DIR` | `/data/Plugins` | Plugin directory |
| `NEO_RPC_PORT` | auto | RPC port override |
| `NEO_LISTEN_PORT` | - | P2P port override |
| `RUST_LOG` | `info` | Log level |

### Monitoring Profile (Grafana)

```bash
# Start with monitoring
docker compose --profile monitoring up -d

# Or use make target
make compose-monitor

# Access Grafana at http://localhost:3000
# Default credentials: admin/admin (or GRAFANA_PASSWORD from .env)
```

### Docker Health Checks

The container includes built-in health checks:

```bash
# Check container health
docker inspect --format='{{.State.Health.Status}}' neo-node

# Manual health check
curl -sf -X POST \
    -H 'Content-Type: application/json' \
    --data '{"jsonrpc":"2.0","id":1,"method":"getversion","params":[]}' \
    http://localhost:20332
```

---

## Running a Node

### Starting the Node

#### Systemd Service (Recommended for Production)

Create `/etc/systemd/system/neo-node.service`:

```ini
[Unit]
Description=Neo N3 Rust Node
After=network.target
Wants=network-online.target

[Service]
Type=simple
User=neo
Group=neo
WorkingDirectory=/opt/neo

# Binary and config
ExecStart=/opt/neo/neo-node --config /opt/neo/neo_production_node.toml

# Restart policy
Restart=always
RestartSec=5
StartLimitInterval=60s
StartLimitBurst=3

# Resource limits
LimitNOFILE=65535
LimitNPROC=8192

# Security
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/neo/data /var/log/neo

# Environment
Environment=RUST_LOG=info
Environment=NEO_PLUGINS_DIR=/var/neo/Plugins

[Install]
WantedBy=multi-user.target
```

Enable and start:
```bash
sudo systemctl daemon-reload
sudo systemctl enable neo-node
sudo systemctl start neo-node
```

#### Direct Execution

```bash
# Basic start
./target/release/neo-node --config neo_mainnet_node.toml

# With custom data directory
./target/release/neo-node \
    --config neo_mainnet_node.toml \
    --storage /var/neo/data

# With logging options
RUST_LOG=info,neo_p2p=debug ./target/release/neo-node \
    --config neo_mainnet_node.toml

# Hardened RPC mode
NEO_RPC_USER=admin NEO_RPC_PASS=$(openssl rand -hex 16) \
    ./target/release/neo-node \
    --config neo_mainnet_node.toml \
    --rpc-hardened
```

### Monitoring

#### Health Check Endpoints

Enable health endpoints with `--health-port`:

```bash
./target/release/neo-node \
    --config neo_mainnet_node.toml \
    --health-port 8080 \
    --health-max-header-lag 20
```

Available endpoints:
- `GET /healthz` - Liveness probe (returns 200 if node is running)
- `GET /readyz` - Readiness probe (returns 200 if synced)
- `GET /metrics` - Prometheus metrics

#### CLI Status Commands

```bash
# Node status
./target/release/neo-cli node status

# Blockchain height
./target/release/neo-cli blockchain height

# Peer information
./target/release/neo-cli node peers

# Check sync status
curl -s -X POST \
    -H 'Content-Type: application/json' \
    --data '{"jsonrpc":"2.0","id":1,"method":"getblockcount","params":[]}' \
    http://localhost:10332
```

#### Log Management

Log configuration in TOML:
```toml
[logging]
level = "info"              # trace, debug, info, warn, error
format = "json"             # json, pretty, compact
file_path = "/var/log/neo/node.log"
max_file_size = "100MB"
max_files = 10
```

Log rotation with logrotate (`/etc/logrotate.d/neo-node`):
```
/var/log/neo/*.log {
    daily
    rotate 14
    compress
    delaycompress
    missingok
    notifempty
    create 0644 neo neo
    sharedscripts
    postrotate
        systemctl reload neo-node || true
    endscript
}
```

Viewing logs:
```bash
# Via systemd
sudo journalctl -u neo-node -f

# Via log file
tail -f /var/log/neo/node.log

# Filter by level
jq 'select(.level == "ERROR")' /var/log/neo/node.log
```

---

## Hardware Requirements

### Minimum Specifications

For running a basic node (syncing and validating):

| Resource | Minimum |
|----------|---------|
| CPU | 2 cores (x86_64) |
| RAM | 4 GB |
| Storage | 100 GB SSD |
| Network | 10 Mbps symmetric |

### Recommended Specifications

For production nodes with RPC enabled:

| Resource | Recommended |
|----------|-------------|
| CPU | 4+ cores (x86_64 or ARM64) |
| RAM | 8 GB |
| Storage | 500 GB NVMe SSD |
| Network | 100 Mbps symmetric |

### Consensus Node Requirements

For nodes participating in dBFT consensus:

| Resource | Requirement |
|----------|-------------|
| CPU | 8+ cores |
| RAM | 16 GB |
| Storage | 1 TB NVMe SSD |
| Network | 1 Gbps dedicated |
| Latency | < 50ms to other CNs |

### Storage Requirements

| Network | Current Size | Growth Rate |
|---------|-------------|-------------|
| MainNet | ~50 GB | ~2 GB/month |
| TestNet | ~30 GB | ~1 GB/month |

Storage breakdown:
- RocksDB data: ~90% of storage
- Logs: ~5-10% (with rotation)
- State caches: ~5%

**Important:** RocksDB requires fast, durable storage. Avoid:
- Network-attached storage (NAS) for primary data
- HDDs (insufficient IOPS)
- tmpfs/ephemeral disks

---

## Upgrading

### Migration Procedures

#### Standard Upgrade

1. **Prepare backup:**
```bash
# Stop the node
sudo systemctl stop neo-node

# Create backup
make backup-rocksdb ROCKSDB_PATH=/var/neo/mainnet BACKUP_DIR=/backups/$(date +%Y%m%d)
```

2. **Deploy new version:**
```bash
# Pull latest code
git fetch origin
git checkout v0.7.1  # or latest tag

# Build new version
cargo build --release -p neo-node -p neo-cli

# Run preflight checks
make preflight
```

3. **Validate and start:**
```bash
# Check configuration compatibility
./target/release/neo-node --config /opt/neo/config.toml --check-all

# Start the node
sudo systemctl start neo-node

# Monitor logs
sudo journalctl -u neo-node -f
```

#### Major Version Upgrade

For breaking changes (check CHANGELOG.md):

1. Export chain data if migration needed
2. Clear data directory if resync required
3. Update configuration schema
4. Deploy and resync from genesis or bootstrap

### Backup/Restore

#### Automated Backup

Using the included script:
```bash
# Daily backup via cron (add to crontab)
0 2 * * * /opt/neo/scripts/backup-rocksdb.sh /var/neo/mainnet /backups

# Or use make target
make backup-rocksdb ROCKSDB_PATH=/var/neo/mainnet BACKUP_DIR=/backups
```

#### Manual Backup

```bash
# Stop node (recommended for consistency)
sudo systemctl stop neo-node

# Create tarball
sudo tar czf /backups/neo-$(date +%Y%m%d).tar.gz /var/neo/mainnet

# Start node
sudo systemctl start neo-node
```

#### Restore from Backup

```bash
# Stop the node
sudo systemctl stop neo-node

# Remove current data (or move aside)
sudo mv /var/neo/mainnet /var/neo/mainnet.old

# Extract backup
sudo tar xzf /backups/neo-20260128.tar.gz -C /

# Fix permissions
sudo chown -R neo:neo /var/neo/mainnet

# Start node
sudo systemctl start neo-node
```

#### Rolling Back

If upgrade fails:
```bash
# Stop current version
sudo systemctl stop neo-node

# Restore previous binaries from backup
sudo cp /backups/neo-node-v0.7.0 /usr/local/bin/neo-node

# Restore data if needed
sudo rm -rf /var/neo/mainnet
sudo tar xzf /backups/neo-pre-upgrade.tar.gz -C /

# Start previous version
sudo systemctl start neo-node
```

---

## Troubleshooting

### Common Issues

#### Node won't start

```bash
# Check configuration
./target/release/neo-node --config /opt/neo/config.toml --check-all

# Verify permissions
ls -la /var/neo/data
ls -la /var/log/neo

# Check logs
sudo journalctl -u neo-node --no-pager -n 50
```

#### Sync is slow

- Check network connectivity to seed nodes
- Verify disk I/O performance
- Increase peer connections in config
- Check for firewall blocking P2P port

#### RPC not responding

```bash
# Test RPC locally
curl -s -X POST \
    -H 'Content-Type: application/json' \
    --data '{"jsonrpc":"2.0","id":1,"method":"getversion","params":[]}' \
    http://127.0.0.1:10332

# Check if RPC is enabled in config
grep -A5 '\[rpc\]' /opt/neo/config.toml
```

### Support Resources

- [GitHub Issues](https://github.com/r3e-network/neo-rs/issues)
- [Operations Guide](./OPERATIONS.md)
- [Monitoring Guide](./MONITORING.md)
- [RPC Hardening Guide](./RPC_HARDENING.md)

---

## Security Checklist

- [ ] Use production profile for builds
- [ ] Enable RPC authentication (`auth_enabled = true`)
- [ ] Disable CORS in production (`cors_enabled = false`)
- [ ] Run as non-root user
- [ ] Configure firewall (P2P port, RPC port)
- [ ] Enable TLS for RPC (via reverse proxy)
- [ ] Restrict RPC bind address to localhost
- [ ] Disable risky RPC methods (`disabled_methods`)
- [ ] Set up log rotation
- [ ] Configure automated backups
- [ ] Enable monitoring and alerting
- [ ] Keep system packages updated

---

## Appendix

### Port Reference

| Network | P2P Port | RPC Port | Usage |
|---------|----------|----------|-------|
| MainNet | 10333 | 10332 | Production network |
| TestNet | 20333 | 20332 | Testing network |
| Private | 30333 | 30332 | Local development |

### File Locations

| Component | Default Path | Configurable |
|-----------|--------------|--------------|
| Binary | `/usr/local/bin/neo-node` | Yes |
| Config | `/etc/neo/` | Yes |
| Data | `/var/neo/data/` | Yes |
| Logs | `/var/log/neo/` | Yes |
| Plugins | `/var/neo/Plugins/` | Yes |
| PID | `/run/neo-node/neo-node.pid` | Via systemd |

### Makefile Reference

| Command | Description |
|---------|-------------|
| `make build-release` | Build release binaries |
| `make compose-up` | Start docker-compose stack |
| `make compose-down` | Stop docker-compose stack |
| `make preflight` | Run config checks |
| `make backup-rocksdb` | Backup RocksDB data |
| `make ci` | Run full CI checks |
