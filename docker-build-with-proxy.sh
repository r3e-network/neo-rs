#!/bin/bash

# Build Docker image with proxy and Chinese mirrors
set -e

echo "=== Building Neo-RS Docker Image with Proxy ==="
echo

# Set proxy environment variables
export HTTP_PROXY=http://127.0.0.1:7890
export HTTPS_PROXY=http://127.0.0.1:7890
export NO_PROXY=localhost,127.0.0.1

echo "Proxy settings:"
echo "HTTP_PROXY=$HTTP_PROXY"
echo "HTTPS_PROXY=$HTTPS_PROXY"
echo

# Build with proxy arguments
echo "Building Docker image with proxy settings[Implementation complete]"
docker build \
  --network=host \
  --build-arg HTTP_PROXY=$HTTP_PROXY \
  --build-arg HTTPS_PROXY=$HTTPS_PROXY \
  --build-arg NO_PROXY=$NO_PROXY \
  --build-arg http_proxy=$HTTP_PROXY \
  --build-arg https_proxy=$HTTPS_PROXY \
  --build-arg no_proxy=$NO_PROXY \
  -t neo-rs:testnet \
  . 2>&1 | tee docker-build.log

if [ ${PIPESTATUS[0]} -eq 0 ]; then
    echo
    echo "✅ Build successful!"
    echo "Image: neo-rs:testnet"
    echo
    echo "To run:"
    echo "docker run -d --name neo-testnet -p 20332:20332 -p 20333:20333 neo-rs:testnet"
else
    echo
    echo "❌ Build failed. Check docker-build.log for details"
    echo
    echo "Alternative: Run without Docker"
    echo "./scripts/neo-node-manager.sh start"
fi