# Running Neo-Rust Node with Docker

This guide explains how to run the Neo-Rust node on testnet using Docker, which helps resolve port conflicts and provides better isolation.

## Prerequisites

- Docker installed on your system
- Docker Compose (optional, for easier management)

## Building the Docker Image

```bash
# Build the Docker image
docker build -t neo-rs:testnet .
```

## Running the Node

### Option 1: Using Docker Run

```bash
# Run on testnet with proper port mapping
docker run -d \
  --name neo-testnet \
  -p 20332:20332 \
  -p 20333:20333 \
  -v neo_testnet_data:/data \
  -e RUST_LOG=info \
  neo-rs:testnet

# View logs
docker logs -f neo-testnet

# Check node status
curl http://localhost:20332/rpc -X POST -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}'
```

### Option 2: Using Docker Compose

```bash
# Start the node using docker-compose
docker-compose -f docker-compose.testnet.yml up -d

# View logs
docker-compose -f docker-compose.testnet.yml logs -f

# Stop the node
docker-compose -f docker-compose.testnet.yml down
```

## Accessing the Node

Once running, you can access:

- **RPC Endpoint**: `http://localhost:20332/rpc`
- **Health Check**: `http://localhost:20332/health`
- **P2P Port**: `20333` (for peer connections)

## Testing RPC Methods

```bash
# Get version
curl -X POST http://localhost:20332/rpc \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getversion","params":[],"id":1}'

# Get block count
curl -X POST http://localhost:20332/rpc \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getblockcount","params":[],"id":1}'

# Get best block hash
curl -X POST http://localhost:20332/rpc \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getbestblockhash","params":[],"id":1}'
```

## Troubleshooting

### Port Conflicts
If you get port binding errors, check what's using the ports:
```bash
lsof -i :20332
lsof -i :20333
```

### Container Issues
```bash
# Check container status
docker ps -a

# View detailed logs
docker logs neo-testnet --tail 100

# Restart container
docker restart neo-testnet

# Remove and recreate
docker stop neo-testnet
docker rm neo-testnet
# Then run the docker run command again
```

### Data Persistence
The Docker setup uses a named volume `neo_testnet_data` to persist blockchain data. To reset:
```bash
docker volume rm neo_testnet_data
```

## Running on Different Networks

### MainNet
```bash
docker run -d \
  --name neo-mainnet \
  -p 10332:10332 \
  -p 10333:10333 \
  -v neo_mainnet_data:/data \
  neo-rs:testnet \
  --mainnet --rpc-port 10332 --p2p-port 10333
```

### Private Network
```bash
docker run -d \
  --name neo-private \
  -p 30332:30332 \
  -p 30333:30333 \
  -v neo_private_data:/data \
  neo-rs:testnet \
  --rpc-port 30332 --p2p-port 30333
```

## Development Tips

1. **Interactive Shell**: Access the container shell
   ```bash
   docker exec -it neo-testnet /bin/bash
   ```

2. **Real-time Logs**: Monitor logs in real-time
   ```bash
   docker logs -f neo-testnet | grep -E "(INFO|ERROR|WARN)"
   ```

3. **Resource Limits**: Limit CPU and memory usage
   ```bash
   docker run -d \
     --name neo-testnet \
     --cpus="2.0" \
     --memory="4g" \
     -p 20332:20332 \
     -p 20333:20333 \
     neo-rs:testnet
   ```

## Notes

- The node will start with only the genesis block (height 0)
- P2P synchronization requires proper network connectivity to testnet seed nodes
- The RPC server will be available immediately, even if P2P is still connecting
- Data is persisted in Docker volumes between container restarts