# Neo N3 Professional Node & CLI

Professional implementation of Neo N3 blockchain node and command-line interface.

## Quick Start

### Running a Node

#### TestNet Node
```bash
# Using environment variables
NEO_NETWORK=testnet NEO_DATA_DIR=./data/testnet neo-node

# Using command line arguments
neo-node --network testnet --data-dir ./data/testnet

# Using configuration file
neo-node --config config/testnet.toml
```

#### MainNet Node
```bash
# Production mainnet node
NEO_NETWORK=mainnet NEO_DATA_DIR=./data/mainnet neo-node

# With custom RPC port
neo-node --network mainnet --rpc-port 10332 --data-dir ./data/mainnet

# Using configuration file
neo-node --config config/mainnet.toml
```

#### Local Development
```bash
# Local development node
neo-node --network local --data-dir ./data/local --log-level debug
```

### Using the CLI Client

#### Node Information
```bash
# Get node version
neo-cli node version

# Get node status
neo-cli node status

# Get connection count
neo-cli node connections

# List connected peers
neo-cli node peers
```

#### Blockchain Queries
```bash
# Get current block height
neo-cli blockchain height

# Get best block hash
neo-cli blockchain best-hash

# Get block by index
neo-cli blockchain block 1000

# Get block by hash (verbose)
neo-cli blockchain block 0x1234... --verbose

# Get transaction
neo-cli blockchain transaction 0xabcd...

# Get account state
neo-cli blockchain account 0x1234...
```

#### Smart Contract Operations
```bash
# Invoke contract method (read-only)
neo-cli contract invoke 0xcontract123 "balanceOf" --params '["0xaddress123"]'

# Get contract state
neo-cli contract state 0xcontract123

# Get contract storage
neo-cli contract storage 0xcontract123 "key"
```

#### Network Information
```bash
# Get network info
neo-cli network info

# Get peer information
neo-cli network peers

# Get mempool status
neo-cli network mempool
```

## Configuration

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `NEO_NETWORK` | Network to connect to (mainnet/testnet/local) | testnet |
| `NEO_CONFIG` | Configuration file path | - |
| `NEO_DATA_DIR` | Data directory | ./data/{network} |
| `NEO_RPC_PORT` | RPC server port | 20332 (testnet) |
| `NEO_P2P_PORT` | P2P network port | 20333 (testnet) |
| `NEO_LOG_LEVEL` | Logging level | info |
| `NEO_METRICS` | Enable metrics endpoint | false |
| `NEO_METRICS_PORT` | Metrics port | 9090 |
| `NEO_CONSENSUS` | Enable consensus | false |
| `NEO_RPC_URL` | RPC server URL for CLI | http://127.0.0.1:20332 |

### Configuration Files

Configuration files are located in the `config/` directory:

- `config/mainnet.toml` - MainNet configuration
- `config/testnet.toml` - TestNet configuration  
- `config/local.toml` - Local development configuration

### Custom Configuration

Create your own configuration file:

```toml
[network]
network_magic = 0x3554334E
address_version = 0x35

[storage]
backend = "rocksdb"
data_dir = "./data/custom"

[p2p]
port = 20333
max_connections = 50
seed_nodes = ["seed1.example.com:20333"]

[rpc]
enabled = true
port = 20332
bind_address = "0.0.0.0"  # Listen on all interfaces
cors_enabled = true

[logging]
level = "info"
format = "json"
file_path = "./logs/neo-node.log"
```

## Command Reference

### neo-node

```bash
neo-node [OPTIONS] [COMMAND]

Commands:
  start     Start the node daemon (default)
  config    Show node configuration
  version   Show version and build info
  validate  Validate configuration file

Options:
  -n, --network <NETWORK>           Network to connect to [default: testnet]
  -c, --config <CONFIG>             Configuration file path
  -d, --data-dir <DATA_DIR>         Data directory
      --rpc-port <RPC_PORT>         RPC server port
      --p2p-port <P2P_PORT>         P2P port
      --log-level <LOG_LEVEL>       Logging level [default: info]
      --metrics                     Enable metrics endpoint
      --metrics-port <METRICS_PORT> Metrics port [default: 9090]
      --no-rpc                      Disable RPC server
      --consensus                   Enable consensus (validator mode)
  -h, --help                        Print help
  -V, --version                     Print version
```

### neo-cli

```bash
neo-cli [OPTIONS] <COMMAND>

Commands:
  node        Node information commands
  blockchain  Blockchain query commands
  wallet      Wallet operations
  contract    Smart contract operations
  network     Network and peer information

Options:
  -r, --rpc-url <RPC_URL>    RPC server URL [default: http://127.0.0.1:20332]
  -f, --format <FORMAT>      Output format [default: json]
  -v, --verbose              Verbose output
  -h, --help                 Print help
  -V, --version              Print version
```

## Examples

### Production MainNet Node

```bash
# Create data directory
mkdir -p /opt/neo/data/mainnet

# Run with production settings
NEO_NETWORK=mainnet \
NEO_DATA_DIR=/opt/neo/data/mainnet \
NEO_LOG_LEVEL=warn \
NEO_METRICS=true \
neo-node
```

### Development TestNet Node

```bash
# Quick testnet node for development
neo-node --network testnet --data-dir ./testnet-data --log-level debug --metrics
```

### Validator Node Setup

```bash
# Enable consensus for validator
neo-node \
  --network mainnet \
  --config config/mainnet.toml \
  --consensus \
  --data-dir /opt/neo/validator
```

### Monitoring and Health Checks

```bash
# Check node status
neo-cli node status

# Monitor block height
watch -n 5 'neo-cli blockchain height'

# Check peer connections
neo-cli node peers | jq '.connected | length'

# Get metrics (if enabled)
curl http://localhost:9090/metrics
```

### RPC API Usage

```bash
# Using different output formats
neo-cli blockchain height --format raw
neo-cli node status --format json
neo-cli network peers --format table

# Verbose output
neo-cli blockchain block 1000 --verbose

# Custom RPC endpoint
neo-cli --rpc-url http://mainnet.example.com:10332 blockchain height
```

## Docker Usage

```bash
# Build image
docker build -t neo-node .

# Run testnet node
docker run -d \
  --name neo-testnet \
  -p 20332:20332 \
  -p 20333:20333 \
  -v neo-testnet-data:/data \
  -e NEO_NETWORK=testnet \
  -e NEO_DATA_DIR=/data \
  neo-node

# Run mainnet node
docker run -d \
  --name neo-mainnet \
  -p 10332:10332 \
  -p 10333:10333 \
  -v neo-mainnet-data:/data \
  -e NEO_NETWORK=mainnet \
  -e NEO_DATA_DIR=/data \
  neo-node
```

## Troubleshooting

### Common Issues

1. **Port already in use**
   ```bash
   # Use different ports
   neo-node --rpc-port 21332 --p2p-port 21333
   ```

2. **Permission denied for data directory**
   ```bash
   # Create directory with proper permissions
   sudo mkdir -p /opt/neo/data
   sudo chown $USER:$USER /opt/neo/data
   ```

3. **Configuration validation**
   ```bash
   # Validate configuration before starting
   neo-node validate config/mainnet.toml
   ```

### Logging

```bash
# Enable debug logging
neo-node --log-level debug

# Log to file
neo-node --log-level info 2>&1 | tee neo-node.log

# JSON structured logging
NEO_LOG_FORMAT=json neo-node
```

### Performance Tuning

```bash
# Increase connection limits
neo-node --network mainnet --max-connections 200

# Disable compression for faster sync
neo-node --network testnet --disable-compression

# Use SSD storage for better performance
neo-node --data-dir /fast-ssd/neo-data
```

## Security Considerations

### Production Deployment

1. **Firewall Configuration**
   - Allow P2P port (10333 for mainnet, 20333 for testnet)
   - Restrict RPC port access (10332/20332) to trusted networks only

2. **RPC Security**
   ```bash
   # Disable dangerous methods in production
   neo-node --config config/mainnet.toml  # Uses disabled_methods in config
   
   # Enable authentication
   NEO_RPC_AUTH=true neo-node
   ```

3. **File Permissions**
   ```bash
   # Secure data directory
   chmod 700 /opt/neo/data
   
   # Secure configuration files
   chmod 600 config/*.toml
   ```

4. **Resource Limits**
   ```bash
   # Limit memory usage
   ulimit -v 8388608  # 8GB virtual memory limit
   
   # Limit file descriptors
   ulimit -n 65536
   ```

## Support

For issues and questions:
- GitHub Issues: https://github.com/r3e-network/neo-rs/issues
- Documentation: https://docs.neo.org
- Community: https://discord.gg/neo
