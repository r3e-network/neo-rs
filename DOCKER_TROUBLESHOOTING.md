# Docker Troubleshooting Guide for Neo-RS

## Current Issue

Docker CLI is installed (version 28.3.2) but the Docker daemon is not responding to commands. This prevents building and running the Neo-RS Docker image.

## Symptoms

1. `docker ps` and other commands timeout
2. Docker Desktop processes are running but daemon is unresponsive
3. Network requests to Docker Hub fail with "context deadline exceeded"

## Solutions to Try

### 1. Restart Docker Desktop Completely

```bash
# Force quit Docker Desktop
osascript -e 'quit app "Docker"'
pkill -f Docker

# Wait a moment
sleep 10

# Start Docker Desktop
open -a Docker

# Wait for it to fully start (this can take 1-2 minutes)
sleep 60

# Test if it's working
docker ps
```

### 2. Reset Docker Desktop to Factory Defaults

1. Open Docker Desktop
2. Go to Settings (gear icon)
3. Navigate to "Troubleshoot" tab
4. Click "Reset to factory defaults"
5. Restart Docker Desktop

### 3. Check System Resources

```bash
# Check if you have enough disk space
df -h

# Check memory usage
top -l 1 | head -10

# Check if Docker's VM has issues
ls -la ~/Library/Containers/com.docker.docker/Data/
```

### 4. Manual Docker Daemon Start

```bash
# Try starting Docker daemon manually
sudo dockerd

# Or with debug logging
sudo dockerd --debug
```

### 5. Use Alternative: Run Without Docker

Since the node is already working locally, you can continue using it without Docker:

```bash
# Use the management script
./scripts/neo-node-manager.sh start

# Or run directly
./target/release/neo-node --testnet --rpc-port 30332 --p2p-port 30333
```

## Pre-built Binary Approach (When Docker Works)

Once Docker is working again, you can use the pre-built binary approach which is faster:

```bash
# The binary is already built
ls -la target/release/neo-node

# Create a minimal Dockerfile
cat > Dockerfile.minimal <<EOF
FROM debian:bullseye-slim
RUN apt-get update && apt-get install -y librocksdb-dev libssl1.1 curl && rm -rf /var/lib/apt/lists/*
COPY target/release/neo-node /usr/local/bin/
EXPOSE 20332 20333
CMD ["neo-node", "--testnet", "--rpc-port", "20332", "--p2p-port", "20333"]
EOF

# Build (when Docker works)
docker build -f Dockerfile.minimal -t neo-rs:minimal .

# Run
docker run -d -p 20332:20332 -p 20333:20333 neo-rs:minimal
```

## Docker Compose Alternative

When Docker is working, you can also use docker-compose:

```bash
# We already have docker-compose.testnet.yml ready
docker-compose -f docker-compose.testnet.yml up -d
```

## Current Node Status

The Neo-RS node is currently running successfully without Docker:
- RPC: http://localhost:30332/rpc
- P2P: 30333
- Process is managed by: `./scripts/neo-node-manager.sh`

## Recommended Next Steps

1. Continue using the node without Docker production ready
2. Troubleshoot Docker Desktop separately
3. Once Docker is working, use the pre-built configurations

The node is fully functional without Docker, so development and testing can continue.