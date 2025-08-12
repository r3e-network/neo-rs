# Neo Rust TestNet Deployment Guide

## Overview

This guide provides step-by-step instructions for deploying a Neo Rust node to the Neo N3 TestNet for integration testing and validation.

## Prerequisites

- Rust 1.70.0 or later
- System with at least 4GB RAM and 50GB disk space
- Stable internet connection
- Basic understanding of blockchain operations

## 🚀 Quick Start

```bash
# Clone and build
git clone https://github.com/r3e-network/neo-rs.git
cd neo-rs
cargo build --release

# Run TestNet node
./target/release/neo-node --testnet
```

## 📋 Detailed Deployment Steps

### 1. System Preparation

#### Ubuntu/Debian
```bash
# Update system
sudo apt update && sudo apt upgrade -y

# Install dependencies
sudo apt install -y build-essential pkg-config libssl-dev librocksdb-dev

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
```

#### macOS
```bash
# Install Homebrew if not already installed
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"

# Install dependencies
brew install rocksdb pkg-config

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
```

### 2. Build Neo Rust Node

```bash
# Clone repository
git clone https://github.com/r3e-network/neo-rs.git
cd neo-rs

# Verify compatibility fixes are applied
./scripts/verify_compatibility.sh

# Build in release mode
cargo build --release --all-features

# Verify build
./target/release/neo-node --version
```

### 3. Configure for TestNet

Create configuration file `testnet-config.toml`:

```toml
[network]
network = "testnet"
magic = 0x74746E41  # TestNet magic number
p2p_port = 20333
rpc_port = 20332
ws_port = 20334
max_peers = 10

# TestNet seed nodes
seed_nodes = [
    "seed1t5.neo.org:20333",
    "seed2t5.neo.org:20333",
    "seed3t5.neo.org:20333",
    "seed4t5.neo.org:20333",
    "seed5t5.neo.org:20333"
]

[storage]
data_dir = "./testnet-data"
cache_size = "2GB"
compression = "lz4"

[rpc]
enabled = true
bind_address = "127.0.0.1:20332"
max_connections = 100
enable_cors = true
cors_origins = ["*"]

[consensus]
enabled = false  # Set to true only for validator nodes

[logging]
level = "info"
format = "json"
file = "./testnet-node.log"

[monitoring]
prometheus_enabled = true
prometheus_port = 9090
```

### 4. Initial Sync

```bash
# Create data directory
mkdir -p testnet-data

# Start node with configuration
./target/release/neo-node --config testnet-config.toml

# Monitor initial sync progress
tail -f testnet-node.log | grep -E "(height|sync|peer)"
```

### 5. Verify Node Operation

#### Check Sync Status
```bash
# Get current block height
curl -X POST http://localhost:20332 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getblockcount","params":[],"id":1}'

# Compare with TestNet explorer
# https://testnet.neoscan.io/
```

#### Check Peer Connections
```bash
# Get connected peers
curl -X POST http://localhost:20332 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getpeers","params":[],"id":1}'
```

#### Monitor Resource Usage
```bash
# Check CPU and memory
htop

# Check disk usage
df -h ./testnet-data

# Check network connections
netstat -an | grep 20333
```

## 🔧 Advanced Configuration

### Running with Docker

Create `Dockerfile.testnet`:

```dockerfile
FROM rust:1.70 as builder

WORKDIR /app
COPY . .
RUN cargo build --release

FROM ubuntu:22.04
RUN apt-get update && apt-get install -y librocksdb-dev ca-certificates
COPY --from=builder /app/target/release/neo-node /usr/local/bin/
COPY testnet-config.toml /etc/neo/config.toml

EXPOSE 20332 20333 20334 9090
VOLUME ["/data"]

CMD ["neo-node", "--config", "/etc/neo/config.toml", "--data-path", "/data"]
```

Build and run:
```bash
docker build -f Dockerfile.testnet -t neo-rust:testnet .
docker run -d \
  --name neo-testnet \
  -p 20332:20332 \
  -p 20333:20333 \
  -v neo-testnet-data:/data \
  neo-rust:testnet
```

### Running with systemd

Create `/etc/systemd/system/neo-testnet.service`:

```ini
[Unit]
Description=Neo Rust TestNet Node
After=network.target

[Service]
Type=simple
User=neo
Group=neo
WorkingDirectory=/opt/neo-rs
ExecStart=/opt/neo-rs/neo-node --config /opt/neo-rs/testnet-config.toml
Restart=always
RestartSec=10

# Security
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/opt/neo-rs/testnet-data

[Install]
WantedBy=multi-user.target
```

Enable and start:
```bash
sudo systemctl daemon-reload
sudo systemctl enable neo-testnet
sudo systemctl start neo-testnet
sudo systemctl status neo-testnet
```

## 📊 Monitoring and Maintenance

### Log Analysis

```bash
# View recent logs
tail -n 100 testnet-node.log

# Check for errors
grep -i error testnet-node.log | tail -20

# Monitor consensus messages
grep -i "consensus\|dbft" testnet-node.log

# Check sync progress
grep "height" testnet-node.log | tail -10
```

### Performance Monitoring

```bash
# Run performance benchmarks
cargo bench --features benchmark

# Monitor with Prometheus
# Access metrics at http://localhost:9090/metrics
```

### Health Checks

Create `health-check.sh`:

```bash
#!/bin/bash

# Check if node is running
if ! pgrep -f neo-node > /dev/null; then
    echo "ERROR: Node is not running"
    exit 1
fi

# Check RPC availability
if ! curl -s http://localhost:20332/health > /dev/null; then
    echo "ERROR: RPC endpoint not responding"
    exit 1
fi

# Check block height
HEIGHT=$(curl -s -X POST http://localhost:20332 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getblockcount","params":[],"id":1}' \
  | jq -r '.result')

echo "Node is healthy. Current height: $HEIGHT"
```

## 🧪 Testing on TestNet

### Send Test Transaction

```bash
# Use neo-cli or compatible wallet to create and send test transactions
# TestNet faucet: https://neowish.ngd.network/
```

### Monitor Your Node

```bash
# Watch for your transactions
curl -X POST http://localhost:20332 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getrawmempool","params":[],"id":1}'
```

## 🔍 Troubleshooting

### Connection Issues

```bash
# Check firewall
sudo ufw status
sudo ufw allow 20333/tcp  # P2P port
sudo ufw allow 20332/tcp  # RPC port

# Test connectivity to seeds
for seed in seed{1..5}t5.neo.org; do
    echo "Testing $seed..."
    nc -zv $seed 20333
done
```

### Sync Issues

```bash
# Clear data and resync
systemctl stop neo-testnet
rm -rf testnet-data/*
systemctl start neo-testnet

# Check disk space
df -h

# Increase peer connections
# Edit config: max_peers = 20
```

### Performance Issues

```bash
# Check resource limits
ulimit -n 65535  # Increase file descriptors

# Optimize RocksDB
# Add to config:
# [storage.rocksdb]
# max_open_files = 5000
# write_buffer_size = "64MB"
```

## 📋 Validation Checklist

- [ ] Node successfully connects to TestNet seeds
- [ ] Block synchronization starts and progresses
- [ ] RPC endpoints respond correctly
- [ ] Consensus messages are received (check logs)
- [ ] Transactions from network are validated
- [ ] No critical errors in logs
- [ ] Resource usage is reasonable
- [ ] Network traffic is as expected

## 🚀 Next Steps

1. **Monitor for 24-48 hours** to ensure stability
2. **Run integration tests** against your node
3. **Test smart contract deployment** (if applicable)
4. **Document any issues** for the development team
5. **Prepare for MainNet** deployment (after thorough testing)

## 📞 Support

- GitHub Issues: https://github.com/r3e-network/neo-rs/issues
- Neo Discord: https://discord.gg/neo
- Documentation: https://docs.rs/neo-rs

---

**Important**: This is a TestNet deployment. Never use TestNet configurations or test keys on MainNet!