# Neo-RS Scripts Documentation

This document describes the utility scripts provided with Neo-RS.

## start-testnet.sh

A convenient launcher script for running a Neo-RS node on TestNet.

### Usage

```bash
./start-testnet.sh
```

### Environment Variables

The script supports the following environment variables for customization:

| Variable | Default | Description |
|----------|---------|-------------|
| `NEO_DATA_DIR` | `/tmp/neo-testnet-data` | Directory for blockchain data storage |
| `NEO_RPC_PORT` | `20332` | Port for RPC server |
| `NEO_P2P_PORT` | `20333` | Port for P2P network connections |
| `NEO_LOG_LEVEL` | `info` | Logging level (trace, debug, info, warn, error) |

### Examples

```bash
# Run with custom data directory
NEO_DATA_DIR=/var/neo/testnet ./start-testnet.sh

# Run with debug logging
NEO_LOG_LEVEL=debug ./start-testnet.sh

# Run with custom ports
NEO_RPC_PORT=8332 NEO_P2P_PORT=8333 ./start-testnet.sh
```

### Features

- **Automatic Cleanup**: Removes stale lock files from previous runs
- **Configuration Display**: Shows all settings before starting
- **Error Checking**: Verifies the neo-node binary exists before running
- **Environment Setup**: Configures logging and error reporting

## Building Neo-RS

Before using the launcher script, you need to build the project:

```bash
# From the project root
cargo build --bin neo-node

# Or from the node directory
cd node && cargo build
```

## TestNet Connection

The node will automatically connect to these TestNet seed nodes:

1. **seed1t5.neo.org** (34.133.235.69:20333)
2. **seed2t5.neo.org** (35.192.59.217:20333)
3. **seed3t5.neo.org** (35.188.199.101:20333)
4. **seed4t5.neo.org** (35.238.26.128:20333)
5. **seed5t5.neo.org** (34.124.145.177:20333)

## Monitoring

Once the node is running, you can monitor it using:

```bash
# Check node health
curl http://localhost:20332/health

# Get current block height
curl -X POST http://localhost:20332 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getblockcount","params":[],"id":1}'

# Get peer connections
curl -X POST http://localhost:20332 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getpeers","params":[],"id":1}'
```

## Troubleshooting

### Port Already in Use

If you get a "port already in use" error:

```bash
# Find process using the port
lsof -i :20333

# Or use different ports
NEO_P2P_PORT=30333 ./start-testnet.sh
```

### Permission Denied

If you get permission errors:

```bash
# Make the script executable
chmod +x start-testnet.sh

# Or run with bash
bash start-testnet.sh
```

### Connection Issues

If the node can't connect to peers:

1. Check firewall settings - ensure outbound connections on port 20333 are allowed
2. Verify network connectivity to the seed nodes
3. Check the log output for specific error messages