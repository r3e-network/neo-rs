# Port Conflict Resolution for Neo-Rust Node

## Option 1: Using Different Ports (Without Docker)

Since the default testnet ports (20332, 20333) are in use, you can run the node on alternative ports:

```bash
# Run on alternative ports
cd /Users/jinghuiliao/git/r3e/neo-rs
cargo run --release --bin neo-node -- --testnet --rpc-port 30332 --p2p-port 30333
```

## Option 2: Find and Stop Conflicting Process

### Find what's using the ports:
```bash
# Check RPC port
lsof -i :20332
# or
sudo netstat -tlnp | grep 20332

# Check P2P port  
lsof -i :20333
# or
sudo netstat -tlnp | grep 20333
```

### Stop the conflicting process:
```bash
# If it's another neo-node instance
pkill -f neo-node

# Or kill by PID (replace PID with actual process ID)
kill -9 PID
```

## Option 3: Docker Installation and Usage

### Install Docker on macOS:
```bash
# Using Homebrew
brew install --cask docker

# Or download from Docker website
# https://www.docker.com/products/docker-desktop
```

### Once Docker is installed:
```bash
# Build the image
docker build -t neo-rs:testnet .

# Run with docker-compose (recommended)
docker-compose -f docker-compose.testnet.yml up -d

# Or run directly
docker run -d \
  --name neo-testnet \
  -p 20332:20332 \
  -p 20333:20333 \
  -v neo_testnet_data:/data \
  neo-rs:testnet
```

## Option 4: Run Multiple Instances with Different Configs

Create separate configuration files for different instances:

### testnet-alt.toml
```toml
[network]
magic = 894710606  # TestNet
p2p_port = 30333
rpc_port = 30332

[rpc]
bind_address = "127.0.0.1:30332"
max_concurrent_connections = 40

[p2p]
bind_address = "0.0.0.0:30333"
max_peers = 10
```

Then run:
```bash
cargo run --release --bin neo-node -- --config testnet-alt.toml
```

## Option 5: Using systemd Socket Activation (Linux)

For Linux systems, you can use systemd socket activation to manage port conflicts:

### /etc/systemd/system/neo-testnet.socket
```ini
[Unit]
Description=Neo TestNet Socket

[Socket]
ListenStream=20332
ListenStream=20333

[Install]
WantedBy=sockets.target
```

### /etc/systemd/system/neo-testnet.service
```ini
[Unit]
Description=Neo TestNet Node
Requires=neo-testnet.socket

[Service]
Type=simple
ExecStart=/usr/local/bin/neo-node --testnet
Restart=on-failure
User=neo

[Install]
WantedBy=multi-user.target
```

## Current Status

The node is currently running on:
- RPC: http://localhost:30332/rpc (working)
- P2P: Port 30333 (attempting to bind)

You can test the RPC endpoint:
```bash
curl -X POST http://localhost:30332/rpc \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}'
```

## Recommended Approach

For development and testing, using alternative ports (Option 1) is the simplest solution. For production deployment, Docker (Option 3) provides the best isolation and portability.