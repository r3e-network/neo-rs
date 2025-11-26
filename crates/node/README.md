# Neo Node

Standalone Neo N3 blockchain node daemon with built-in RPC server.

## Overview

`neo-node` is a standalone daemon that runs the Neo N3 blockchain node. It:
- Synchronizes with the Neo network
- Provides a JSON-RPC API for external clients
- Manages the blockchain database
- Supports plugins (RpcServer, dBFT, ApplicationLogs, etc.)

## Installation

```bash
cargo build -p neo-node --release
```

## Usage

```bash
# Start with default configuration (mainnet)
neo-node --config neo_mainnet_node.toml

# Start with testnet configuration
neo-node --config neo_testnet_node.toml

# Run in daemon mode (minimal console output)
neo-node --config neo_mainnet_node.toml --daemon

# Override storage path
neo-node --config neo_mainnet_node.toml --storage ./data/chain

# Use RocksDB backend
neo-node --config neo_mainnet_node.toml --backend rocksdb --storage ./data/chain
```

## Command-line Options

| Option | Description | Default |
|--------|-------------|--------|
| `-c, --config <PATH>` | Path to TOML configuration file | `neo_mainnet_node.toml` |
| `--storage <PATH>` | Override storage path | (from config) |
| `--backend <NAME>` | Storage backend (memory, rocksdb) | (from config) |
| `--network-magic <N>` | Override network magic | (from config) |
| `--listen-port <PORT>` | P2P listening port | (from config) |
| `--seed <HOST:PORT>` | Seed nodes (comma separated) | (from config) |
| `--max-connections <N>` | Maximum connections | (from config) |
| `--min-connections <N>` | Minimum desired peers | (from config) |
| `-d, --daemon` | Daemon mode (no console output) | false |

## Configuration

See `neo_mainnet_node.toml` for a full configuration example. Key sections:

```toml
[network]
network_type = "mainnet"  # or "testnet", "privatenet"

[p2p]
listen_port = 10333
max_connections = 40
seed_nodes = ["seed1.neo.org:10333", "seed2.neo.org:10333"]

[storage]
path = "./data/chain"
backend = "rocksdb"

[rpc]
enabled = true
bind_address = "127.0.0.1"
port = 10332

[logging]
active = true
level = "info"
console_output = true
```

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                      neo-node                            │
│  ┌─────────────────────────────────────────────────────┐│
│  │                   NeoSystem                         ││
│  │  ┌────────────┐  ┌────────────┐  ┌────────────┐   ││
│  │  │ Blockchain │  │ LocalNode  │  │TaskManager │   ││
│  │  │   Actor    │  │   Actor    │  │   Actor    │   ││
│  │  └────────────┘  └────────────┘  └────────────┘   ││
│  └─────────────────────────────────────────────────────┘│
│  ┌─────────────────────────────────────────────────────┐│
│  │                    Plugins                          ││
│  │  ┌────────────┐  ┌────────────┐  ┌────────────┐   ││
│  │  │ RpcServer  │  │   dBFT     │  │ AppLogs    │   ││
│  │  └────────────┘  └────────────┘  └────────────┘   ││
│  └─────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────┘
                           │
                           │ JSON-RPC
                           ▼
                    ┌─────────────┐
                    │   neo-cli   │
                    └─────────────┘
```

## License

MIT License - see LICENSE file in the repository root.
