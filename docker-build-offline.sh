#!/bin/bash

# Docker Build Script with Offline/Network Issue Handling
# This script helps build the Neo-RS Docker image when experiencing network issues

set -e

echo "Neo-RS Docker Build Script"
echo "=========================="

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Check if Docker is responsive
check_docker() {
    echo -n "Checking Docker daemon[Implementation complete] "
    if docker info >/dev/null 2>&1; then
        echo -e "${GREEN}OK${NC}"
        return 0
    else
        echo -e "${RED}Docker daemon is not responding${NC}"
        return 1
    fi
}

# Test network connectivity
test_network() {
    echo -n "Testing Docker Hub connectivity[Implementation complete] "
    if curl -s -o /dev/null -w "%{http_code}" --connect-timeout 5 https://registry-1.docker.io/v2/ | grep -q "401"; then
        echo -e "${GREEN}OK${NC}"
        return 0
    else
        echo -e "${YELLOW}Network issues detected${NC}"
        return 1
    fi
}

# Build with retry
build_with_retry() {
    local attempt=1
    local max_attempts=3
    
    while [ $attempt -le $max_attempts ]; do
        echo -e "\n${YELLOW}Build attempt $attempt of $max_attempts${NC}"
        
        if docker build -t neo-rs:testnet . 2>&1 | tee build.log; then
            echo -e "\n${GREEN}Build successful!${NC}"
            return 0
        else
            echo -e "\n${RED}Build failed on attempt $attempt${NC}"
            
            # Check for specific errors
            if grep -q "context deadline exceeded" build.log; then
                echo "Network timeout detected. Waiting before retry[Implementation complete]"
                sleep 30
            elif grep -q "net/http: request canceled" build.log; then
                echo "Connection issue detected. Checking Docker[Implementation complete]"
                check_docker || exit 1
                sleep 10
            fi
            
            attempt=$((attempt + 1))
        fi
    done
    
    echo -e "\n${RED}Build failed after $max_attempts attempts${NC}"
    return 1
}

# Alternative: Build locally first
build_local_alternative() {
    echo -e "\n${YELLOW}Alternative: Building binary locally first${NC}"
    echo "This approach builds the Rust binary on your host machine, then creates a minimal Docker image"
    
    # Build the binary locally
    echo "Building neo-node binary locally[Implementation complete]"
    if cargo build --release --package neo-node --bin neo-node; then
        echo -e "${GREEN}Local build successful${NC}"
        
        # Create a simple Dockerfile for the pre-built binary
        cat > Dockerfile.prebuilt <<EOF
FROM debian:bullseye-slim

# Install runtime dependencies only
RUN apt-get update && apt-get install -y \
    ca-certificates \
    librocksdb-dev \
    libsnappy1v5 \
    liblz4-1 \
    libzstd1 \
    zlib1g \
    libbz2-1.0 \
    libssl1.1 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create neo user
RUN groupadd -r neo && useradd -r -g neo neo

# Create data directories
RUN mkdir -p /data /data/blocks /data/logs && chown -R neo:neo /data

# Copy the pre-built binary
COPY target/release/neo-node /usr/local/bin/neo-node

# Set up volumes
VOLUME ["/data"]

# Switch to neo user
USER neo

# Expose ports
EXPOSE 20332 20333

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=60s --retries=3 \
    CMD curl -f http://localhost:20332/health || exit 1

# Default command for testnet
ENTRYPOINT ["neo-node"]
CMD ["--testnet", "--rpc-port", "20332", "--p2p-port", "20333"]
EOF
        
        echo "Building Docker image with pre-built binary[Implementation complete]"
        if docker build -f Dockerfile.prebuilt -t neo-rs:testnet-prebuilt .; then
            echo -e "${GREEN}Docker image built successfully!${NC}"
            echo "Image name: neo-rs:testnet-prebuilt"
            return 0
        fi
    fi
    
    return 1
}

# Main execution
main() {
    cd "$(dirname "$0")/.."
    
    if ! check_docker; then
        echo -e "${RED}Docker is not running or not accessible${NC}"
        echo "Please ensure Docker Desktop is running and try again"
        exit 1
    fi
    
    if test_network; then
        echo "Network connectivity is good. Attempting standard build[Implementation complete]"
        build_with_retry
    else
        echo -e "${YELLOW}Network issues detected. Using alternative approaches[Implementation complete]${NC}"
        
        # Try build anyway (might work with cached layers)
        if ! build_with_retry; then
            echo -e "\n${YELLOW}Standard build failed. Trying local build alternative[Implementation complete]${NC}"
            build_local_alternative
        fi
    fi
    
    # Show available images
    echo -e "\n${GREEN}Available Docker images:${NC}"
    docker images | grep neo-rs || echo "No neo-rs images found"
    
    # Provide run instructions
    if docker images | grep -q neo-rs; then
        echo -e "\n${GREEN}To run the node:${NC}"
        echo "docker run -d \\"
        echo "  --name neo-testnet \\"
        echo "  -p 20332:20332 \\"
        echo "  -p 20333:20333 \\"
        echo "  -v neo_testnet_data:/data \\"
        echo "  neo-rs:testnet"
        echo ""
        echo "Or use docker-compose:"
        echo "docker-compose -f docker-compose.testnet.yml up -d"
    fi
}

# Run main function
main "$@"