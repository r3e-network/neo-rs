# Neo Node

Standalone Neo N3 blockchain node daemon with built-in RPC server.

## Overview

`neo-node` is a standalone daemon that runs the Neo N3 blockchain node. It:
- Synchronizes with the Neo network over the Neo P2P protocol
- Provides a JSON-RPC API for external clients
- Manages the blockchain database (MDBX default, RocksDB fallback)
- Supports built-in services (RpcServer, NeoIndexer, ApplicationLogs, TokensTracker, StateService, OracleService when enabled)
- Consensus (dBFT 2.0) can be enabled via DBFTPlugin settings and a validator wallet
- Optional TEE support (SGX/Nitro) and HSM-backed consensus signing

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
- `--storage-path` uses the configured persistent backend, defaulting to MDBX in production builds, and overrides `[storage].data_dir` / `[storage].path`.
- When dBFT is enabled, the validator key comes from the `[consensus]` configuration.

## Command-line Options

| Option | Description | Default |
|--------|-------------|--------|
| `-c, --config <PATH>` | Path to TOML configuration file | `neo_testnet_node.toml` |
| `--storage-path <PATH>` | Override storage path for the configured/default persistent backend | (from config) |
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
data_dir = "./data/chain"
backend = "mdbx"
static_files_dir = "./data/chain-static"

[rpc]
enabled = true
bind_address = "127.0.0.1"
port = 10332

[indexer]
enabled = true
store_path = "./data/mainnet/indexer"
backfill_on_startup = true

[application_logs]
enabled = true
path = "ApplicationLogs_{0}"

[tokens_tracker]
enabled = true
db_path = "TokensTracker_{0}"
enabled_trackers = ["NEP-17", "NEP-11"]

[telemetry.metrics]
enabled = true
bind_address = "127.0.0.1"
port = 9090
path = "/metrics"

[observability]
enabled = false
service_name = "neo-node-mainnet"
environment = "production"

[logging]
active = true
level = "info"
console_output = true
```

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                      neo-node (L7)                       │
│  ┌─────────────────────────────────────────────────────┐│
│  │              neo-system (L5 Composition)             ││
│  │  ┌────────────┐  ┌────────────┐  ┌────────────┐    ││
│  │  │ Blockchain │  │ LocalNode  │  │ Supervisor │    ││
│  │  │  Service   │  │  (P2P)     │  │ (Tasks)    │    ││
│  │  └────────────┘  └────────────┘  └────────────┘    ││
│  └─────────────────────────────────────────────────────┘│
│  ┌─────────────────────────────────────────────────────┐│
│  │           neo-rpc + neo-oracle-service (L6)         ││
│  │  ┌────────────┐  ┌────────────┐  ┌────────────┐    ││
│  │  │ RpcServer  │  │ NeoIndexer │  │ AppLogs    │    ││
│  │  └────────────┘  └────────────┘  └────────────┘    ││
│  │  ┌────────────┐  ┌────────────┐  ┌────────────┐    ││
│  │  │TokenTrack  │  │StateRoot   │  │ Oracle     │    ││
│  │  └────────────┘  └────────────┘  └────────────┘    ││
│  └─────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────┘
                           │
                           │ JSON-RPC
                           ▼
                    ┌──────────────────┐
                    │ JSON-RPC clients │
                    └──────────────────┘
```

The node follows a 7-layer architecture inspired by **reth** (provider traits,
type-state `NodeComponents`, `EngineApi`) and **Polkadot/Substrate** (bounded
context layers, per-domain error types). See `../design.md` for the 15 ADRs
and the evolution roadmap.

## License

MIT License - see LICENSE file in the repository root.
