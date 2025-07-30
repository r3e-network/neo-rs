#!/bin/bash

# Test Docker with Chinese Mirrors
set -e

echo "=== Testing Docker with Chinese Mirrors ==="
echo

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Wait for Docker to be ready
echo "Waiting for Docker to start[Implementation complete]"
COUNTER=0
MAX_TRIES=30
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
    echo -e "${RED}Docker failed to start${NC}"
    echo "Please start Docker Desktop manually and run this script again"
    exit 1
fi

# Show current configuration
echo "Current Docker configuration:"
echo "Registry mirrors configured in daemon.json:"
cat ~/.docker/daemon.json | grep -A 5 "registry-mirrors" || echo "No mirrors configured"

# Test pulling a small image
echo
echo "Testing image pull with Chinese mirrors[Implementation complete]"
echo "Pulling alpine:latest[Implementation complete]"
if docker pull alpine:latest; then
    echo -e "${GREEN}Successfully pulled alpine:latest!${NC}"
    
    # Now try building Neo-RS
    echo
    echo "Attempting to build Neo-RS Docker image[Implementation complete]"
    cd "$(dirname "$0")"
    
    if docker build -t neo-rs:testnet .; then
        echo -e "${GREEN}Successfully built neo-rs:testnet!${NC}"
        
        echo
        echo "Available images:"
        docker images | grep -E "(neo-rs|REPOSITORY)" | head -5
        
        echo
        echo "To run the node:"
        echo "docker run -d --name neo-testnet -p 20332:20332 -p 20333:20333 neo-rs:testnet"
        echo "Or use: docker-compose -f docker-compose.testnet.yml up -d"
    else
        echo -e "${RED}Failed to build Neo-RS image${NC}"
    fi
else
    echo -e "${RED}Failed to pull alpine image${NC}"
    echo
    echo "Troubleshooting:"
    echo "1. Check if proxy is blocking Docker"
    echo "2. Try setting proxy in Docker Desktop settings"
    echo "3. Check network connectivity: curl https://registry.docker-cn.com"
fi