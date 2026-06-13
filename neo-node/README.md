# Neo Node

Standalone Neo N3 blockchain node daemon with built-in RPC server.

## Overview

`neo-node` is a standalone daemon that runs the Neo N3 blockchain node. It:
- Synchronizes with the Neo network
- Provides a JSON-RPC API for external clients
- Manages the blockchain database
- Supports built-in services (RpcServer, ApplicationLogs, TokensTracker, StateService when enabled). Consensus (dBFT) can be enabled via DBFTPlugin settings and a validator wallet.

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

# Override storage path
neo-node --config neo_mainnet_node.toml --storage-path ./data/chain

# Validate configuration and storage without starting P2P/RPC
neo-node --config neo_mainnet_node.toml --check-all

# Override network magic for a private network
neo-node --config custom.toml --network-magic 123456
```

Notes:
- Storage backend, P2P, RPC, and consensus settings live in TOML.
- `--storage-path` implies the RocksDB backend and overrides `[storage].data_dir` / `[storage].path`.
- When dBFT is enabled, the validator key comes from the `[consensus]` configuration.

## Command-line Options

| Option | Description | Default |
|--------|-------------|--------|
| `-c, --config <PATH>` | Path to TOML configuration file | `neo_testnet_node.toml` |
| `--storage-path <PATH>` | Override storage path and use RocksDB | (from config) |
| `--network-magic <N>` | Override network magic | (from config) |
| `--check-config` | Validate configuration and exit | false |
| `--check-storage` | Validate storage can be opened and exit | false |
| `--check-all` | Run all preflight checks and exit | false |

## Configuration

See `neo_mainnet_node.toml` for a full configuration example. Key sections:

```toml
[network]
network_type = "mainnet"  # or "testnet", "privatenet"

[p2p]
port = 10333
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
│  │  │ RpcServer  │  │ AppLogs    │  │ TokenTrack │   ││
│  │  └────────────┘  └────────────┘  └────────────┘   ││
│  └─────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────┘
                           │
                           │ JSON-RPC
                           ▼
                    ┌──────────────────┐
                    │ JSON-RPC clients │
                    └──────────────────┘
```

## License

MIT License - see LICENSE file in the repository root.
