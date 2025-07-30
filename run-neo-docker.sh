#!/bin/bash
# Run Neo-RS in Docker

IMAGE_NAME="neo-rs:offline"

# Check if we have any neo-rs image
if docker images | grep -q "neo-rs"; then
    # Use the first available neo-rs image
    IMAGE_NAME=$(docker images | grep "neo-rs" | head -1 | awk '{print $1":"$2}')
    echo "Using image: $IMAGE_NAME"
else
    echo "No neo-rs image found!"
    exit 1
fi

# Run the container
docker run -d \
    --name neo-testnet \
    -p 20332:20332 \
    -p 20333:20333 \
    -v neo_data:/data \
    $IMAGE_NAME \
    /usr/local/bin/neo-node --testnet --rpc-port 20332 --p2p-port 20333

echo "Neo-RS container started!"
echo "Check logs: docker logs -f neo-testnet"
