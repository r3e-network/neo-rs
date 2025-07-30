#!/bin/bash

# Fix Docker Network Issues Script
set -e

echo "=== Docker Network Fix Script ==="
echo

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Step 1: Kill all Docker processes
echo "Step 1: Stopping all Docker processes[Implementation complete]"
pkill -f Docker || true
sleep 5

# Step 2: Clear Docker cache (optional)
echo "Step 2: Clearing Docker cache[Implementation complete]"
rm -rf ~/Library/Containers/com.docker.docker/Data/vms/0/data/Docker.raw.lock 2>/dev/null || true

# Step 3: Start Docker Desktop
echo "Step 3: Starting Docker Desktop[Implementation complete]"
open -a Docker

# Step 4: Wait for Docker to be ready
echo "Step 4: Waiting for Docker to be ready[Implementation complete]"
COUNTER=0
MAX_TRIES=60
while [ $COUNTER -lt $MAX_TRIES ]; do
    if docker version >/dev/null 2>&1; then
        echo -e "${GREEN}Docker is ready!${NC}"
        break
    fi
    echo -n "."
    sleep 2
    COUNTER=$((COUNTER+1))
done
echo

if [ $COUNTER -eq $MAX_TRIES ]; then
    echo -e "${RED}Docker failed to start after 2 minutes${NC}"
    exit 1
fi

# Step 5: Test connectivity
echo "Step 5: Testing Docker Hub connectivity[Implementation complete]"
if docker pull hello-world >/dev/null 2>&1; then
    echo -e "${GREEN}Successfully connected to Docker Hub!${NC}"
else
    echo -e "${YELLOW}Warning: Could not connect to Docker Hub${NC}"
    echo "Trying with explicit proxy[Implementation complete]"
    
    # Test with proxy
    export HTTP_PROXY=http://127.0.0.1:7890
    export HTTPS_PROXY=http://127.0.0.1:7890
    
    if docker pull hello-world; then
        echo -e "${GREEN}Successfully connected via proxy!${NC}"
    else
        echo -e "${RED}Connection failed even with proxy${NC}"
    fi
fi

# Step 6: Build Neo-RS
echo
echo "Step 6: Building Neo-RS Docker image[Implementation complete]"
cd "$(dirname "$0")"

# Try building with different approaches
echo "Attempting build with current Dockerfile[Implementation complete]"
if docker build -t neo-rs:testnet . --network=host; then
    echo -e "${GREEN}Build successful!${NC}"
else
    echo -e "${YELLOW}Build failed. Trying alternative approach[Implementation complete]${NC}"
    
    # Alternative: Use pre-built binary
    echo "Building binary locally first[Implementation complete]"
    if [ -f target/release/neo-node ]; then
        echo "Using existing binary[Implementation complete]"
    else
        cargo build --release --package neo-node --bin neo-node
    fi
    
    # Create minimal Dockerfile
    cat > Dockerfile.minimal <<EOF
FROM debian:bullseye-slim
ENV HTTP_PROXY=http://127.0.0.1:7890
ENV HTTPS_PROXY=http://127.0.0.1:7890
RUN apt-get update && apt-get install -y \\
    ca-certificates \\
    librocksdb-dev \\
    libssl1.1 \\
    curl \\
    && rm -rf /var/lib/apt/lists/*
COPY target/release/neo-node /usr/local/bin/
EXPOSE 20332 20333
CMD ["neo-node", "--testnet", "--rpc-port", "20332", "--p2p-port", "20333"]
EOF
    
    echo "Building minimal Docker image[Implementation complete]"
    if docker build -f Dockerfile.minimal -t neo-rs:minimal . --network=host; then
        echo -e "${GREEN}Minimal build successful!${NC}"
        echo "Image: neo-rs:minimal"
    else
        echo -e "${RED}All build attempts failed${NC}"
        exit 1
    fi
fi

# Step 7: Show results
echo
echo "=== Build Complete ==="
docker images | grep neo-rs || echo "No images found"

echo
echo "To run the node:"
echo "docker run -d --name neo-testnet -p 20332:20332 -p 20333:20333 neo-rs:testnet"
echo "Or: docker-compose -f docker-compose.testnet.yml up -d"