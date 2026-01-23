# neo-rs

Professional Rust implementation of the Neo N3 blockchain node and CLI tools.

[![Build Status](https://github.com/r3e-network/neo-rs/workflows/CI/badge.svg)](https://github.com/r3e-network/neo-rs/actions)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust Version](https://img.shields.io/badge/rust-1.75+-blue.svg)](https://www.rust-lang.org)

## Quick Start

### Installation

```bash
# Clone the repository
git clone https://github.com/r3e-network/neo-rs.git
cd neo-rs

# Build all components
cargo build --release
```

### Running a Node

#### Simple Usage
```bash
# TestNet node (default)
./target/release/neo-node

# MainNet node
./target/release/neo-node --network mainnet

# Local development node
./target/release/neo-node --network local --log-level debug
```

#### Advanced Usage
```bash
# Using environment variables
NEO_NETWORK=testnet NEO_DATA_DIR=./data/testnet ./target/release/neo-node

# Using configuration file
./target/release/neo-node --config config/testnet.toml

# Production mainnet with metrics
./target/release/neo-node --network mainnet --data-dir /opt/neo/data --metrics
```

### Using the CLI Client

```bash
# Get node status
./target/release/neo-cli node status

# Get current block height
./target/release/neo-cli blockchain height

# Get block information
./target/release/neo-cli blockchain block 1000 --verbose

# Invoke smart contract (read-only)
./target/release/neo-cli contract invoke 0xcontract123 "balanceOf" --params '["0xaddress123"]'
```

📖 **See [CLI Usage Guide](docs/CLI_USAGE.md) for comprehensive documentation.**

## Documentation

| Document | Description |
|----------|-------------|
| [Architecture](./docs/ARCHITECTURE.md) | Neo system architecture and crate boundaries |
| [Architecture Comparison](./docs/ARCHITECTURE_COMPARISON.md) | C# vs Rust implementation differences |
| [Plugin System](./docs/PLUGIN_SYSTEM.md) | Plugin architecture and adding new services |
| [CLI Architecture](./docs/CLI_ARCHITECTURE.md) | CLI design and wallet operation model |
| [Deployment](./docs/DEPLOYMENT.md) | Production deployment guide |
| [Operations](./docs/OPERATIONS.md) | Day-to-day operations and maintenance |

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Application Layer                        │
│  neo-cli (CLI Client)     │  neo-node (Node Daemon)         │
├─────────────────────────────────────────────────────────────┤
│                    Chain Management                         │
│  neo-chain (Blockchain)   │  neo-mempool (Transaction Pool) │
├─────────────────────────────────────────────────────────────┤
│                    Core Layer                               │
│  neo-core (Core Logic)  │  neo-vm (Virtual Machine)         │
│  neo-consensus (dBFT)   │  neo-p2p (P2P Network)            │
│  neo-rpc (RPC Server)                                       │
├─────────────────────────────────────────────────────────────┤
│                    Foundation Layer                         │
│  neo-primitives │ neo-crypto │ neo-storage │ neo-io │ neo-json│
└─────────────────────────────────────────────────────────────┘
```

## Compatibility

| neo-rs Version | Neo N3 Version | C# Reference                                                                                      |
| -------------- | -------------- | ------------------------------------------------------------------------------------------------- |
| 0.7.x          | 3.9.2          | [`71c2f8e`](https://github.com/neo-project/neo/commit/71c2f8e65274b9054cf1011f06fb80b078e3e631) (v3.9.2+ production ready) |
| 0.4.x          | 3.8.2          | [`ede620e`](https://github.com/neo-project/neo/commit/ede620e5722c48e199a0f3f2ab482ae090c1b878) |

This implementation maintains byte-for-byte serialization compatibility with the official C# Neo implementation (v3.9.2) for blocks, transactions, and P2P messages. Verified against commit `71c2f8e` (neo_csharp submodule) for semantic parity.

### C# v3.9.2 Feature Parity

The following C# Neo v3.9.2 features are fully implemented:

| Feature | Status | Description |
|---------|--------|-------------|
| **VersionPayload NodeKey/NodeId** | ✅ Complete | P2P identity using ECDSA public key + SHA256(node_id) |
| **P2P Signature Verification** | ✅ Complete | VersionPayload cryptographic signature for handshake |
| **BIP-0032 HD Wallets** | ✅ Complete | ExtendedKey, KeyPath derivation (m/44'/888'/i'/0/0) |
| **BIP-0039 Mnemonics** | ✅ Complete | Multi-language wordlists (10 languages) |
| **NEP-30 Oracle/Notary/Treasury** | ✅ Complete | NEP-30 standard support for native contracts |
| **TokenManagement Contract** | ✅ Complete | NEP-17/NEP-11 management with `_onTransfer` callbacks |
| **TokenManagement Methods** | ✅ Complete | create, mint, burn, transfer, balanceOf, getTokenInfo, getAssetsOfOwner |
| **Fungible Token (NEP-17)** | ✅ Complete | Full implementation with mintable_address validation |
| **Gas Token (NEP-17)** | ✅ Complete | Mint, burn, transfer with `onNEP17Payment` callback |
| **Neo Token (NEP-17)** | ✅ Complete | Voting, candidate registration, GAS distribution |
| **Notary Contract** | ✅ Complete | Multi-signature deposits, GAS locking |
| **Oracle Contract** | ✅ Complete | External data requests with NEP-30 support |
| **Policy Contract** | ✅ Complete | Fee management, account blocking |
| **Role Management** | ✅ Complete | Oracle/Notary role designation |
| **Ledger Contract** | ✅ Complete | Block/transaction storage, state roots |
| **StdLib Crypto** | ✅ Complete | SHA256, RIPEMD160, BLS12-381, Keccak256 |
| **Base58 Security** | ✅ Complete | Stack allocation bounds checking |

### Native Contract IDs

All native contract hashes match the C# reference implementation:

| Contract | ID | Hash (LE) |
|----------|---|-----------|
| ContractManagement | -1 | `0xfffdc93764dbaddd97c48f252a53ea4643faa3fd` |
| StdLib | -2 | `0xacce6fd80d44e1796aa0c2c625e9e4e0ce39efc0` |
| CryptoLib | -3 | `0x726cb6e0cd8628a1350a611384688911ab75f51b` |
| LedgerContract | -4 | `0xda65b600f7124ce6c79950c1772a36403104f2be` |
| NeoToken | -5 | `0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5` |
| GasToken | -6 | `0xd2a4cff31913016155e38e474a2c06d08be276cf` |
| PolicyContract | -7 | `0xcc5e4edd9f5f8dba8bb65734541df7a1c081c67b` |
| RoleManagement | -8 | `0x49cf4e5378ffcd4dec034fd98a174c5491e395e2` |
| OracleContract | -9 | `0xfe924b7cfe89ddd271abaf7210a80a7e11178758` |
| Notary | -10 | `0xc1e14f19c3e60d0b9244d06dd7ba9b113135ec3b` |
| Treasury | -11 | `0x156326f25b1b5d839a4d326aeaa75383c9563ac1` |
| TokenManagement | -12 | `0xae00c57daeb20f9b65504f53265e4f32b9f4a8a0` |

### Test Coverage

```
✅ 313 lib tests passed (neo-core)
✅ 520+ integration tests passed
✅ All C# UT_* equivalent tests converted to Rust
✅ JSON manifest parity with C# reference (byte-for-byte)
✅ Contract hash verification (all 12 native contracts)
✅ NEP-17 Transfer/NEP-30 Oracle callbacks tested
```

## Prerequisites

- Rust (stable toolchain recommended)
- RocksDB native library (required by the default storage provider). On Ubuntu/Debian: `sudo apt-get install librocksdb-dev`.

## Build

```bash
cargo build --workspace
```

Release build for production:

```bash
cargo build --workspace --release
```

## Run the node

`neo-node` is the daemon (P2P sync + optional JSON-RPC server). `neo-cli` is a JSON-RPC client.

```bash
cargo run -p neo-node --release -- --config neo_mainnet_node.toml
```

Common overrides:

- `--storage <path>`: custom RocksDB path
- `--backend <memory|rocksdb>`: storage backend
- `--network-magic <u32>` / `--listen-port <u16>`: network parameters

Use `cargo run -p neo-node -- --help` for the full daemon flag list.

Query a running node:

```bash
cargo run -p neo-cli --release -- --rpc-url http://localhost:10332 state
```

Validate a node config without starting the daemon:

```bash
cargo run -p neo-node -- --config neo_mainnet_node.toml --check-config
```

Validate storage accessibility without starting the daemon:

```bash
cargo run -p neo-node -- --config neo_mainnet_node.toml --check-storage
```

Run both checks in one go:

```bash
cargo run -p neo-node -- --config neo_mainnet_node.toml --check-all
```

Preflight both bundled configs:

```bash
make preflight
```

Environment overrides:

- `NEO_CONFIG` (path to TOML), `NEO_STORAGE` (data path), `NEO_BACKEND` (storage backend)
- `NEO_STORAGE_READONLY` (open storage read-only; use with `--check-*` only)
- `NEO_NETWORK_MAGIC`, `NEO_LISTEN_PORT`, `NEO_SEED_NODES`
- `NEO_MAX_CONNECTIONS`, `NEO_MIN_CONNECTIONS`, `NEO_MAX_CONNECTIONS_PER_ADDRESS`, `NEO_BROADCAST_HISTORY_LIMIT`
- `NEO_BLOCK_TIME`, `NEO_DISABLE_COMPRESSION`, `NEO_DAEMON`
- `NEO_RPC_BIND`, `NEO_RPC_PORT`, `NEO_RPC_DISABLE_CORS`, `NEO_RPC_USER`, `NEO_RPC_PASS`, `NEO_RPC_TLS_CERT`, `NEO_RPC_TLS_PASS`
- `NEO_RPC_ALLOW_ORIGINS`, `NEO_RPC_DISABLED_METHODS`
- `NEO_LOG_PATH`, `NEO_LOG_LEVEL`, `NEO_LOG_FORMAT`
- `NEO_STATE_ROOT` to enable state root calculation/validation (`--state-root`/`--stateroot`)
- `NEO_STATE_ROOT_PATH` to choose the StateRoot DB path (defaults to `<storage>/StateRoot`)
- `NEO_STATE_ROOT_FULL_STATE` to keep full historical state (enables old-root proofs; larger DB)
- `NEO_HEALTH_PORT` to expose `/healthz` on localhost
- `NEO_HEALTH_MAX_HEADER_LAG` to fail `/healthz` if header lag exceeds the threshold (defaults to 20; set to 0 to disable)
- `/metrics` is available when the health server is enabled; scrape it with Prometheus.
- `/readyz` is available when the health server is enabled (same contract as `/healthz`).

Hardened RPC preset:

- Use `--rpc-hardened` (or set via CLI) to disable CORS, require auth, and disable `openwallet`/`listplugins` by default; combine with `NEO_RPC_USER/NEO_RPC_PASS`.

Example hardened run:

```bash
NEO_RPC_USER=admin NEO_RPC_PASS="$(openssl rand -hex 16)" \
NEO_RPC_BIND=127.0.0.1 NEO_RPC_PORT=10332 \
cargo run -p neo-node -- --config neo_mainnet_node.toml --rpc-hardened --check-all
```

## Docker

Build an image and run on TestNet with a persistent data volume:

```bash
docker build -t neo-rs .
docker run -d --name neo-node \
  -p 20332:20332 -p 20333:20333 \
  -v $(pwd)/data:/data \
  -e NEO_NETWORK=testnet \
  neo-rs
```

Key environment knobs:

- `NEO_NETWORK`: `testnet` (default) or `mainnet` to pick the bundled TOML config.
- `NEO_STORAGE`: RocksDB path inside the container (defaults to `/data/testnet` or `/data/mainnet` based on `NEO_NETWORK`).
- `NEO_CONFIG`: custom config path if you bind-mount your own TOML.
- `NEO_PLUGINS_DIR`: where plugin configs (e.g., RpcServer.json) are written; defaults to `/data/Plugins`.
- `NEO_BACKEND`: storage backend passed to `--backend` (default `rocksdb` in Docker/compose).
- `NEO_RPC_PORT`: if set, forces the RPC port (used by the health check). Otherwise the entrypoint will try to read the port from the TOML `[rpc]` section and fall back to network defaults.
- `NEO_LISTEN_PORT`: override the P2P listen port without editing the TOML.
- Containers run as an unprivileged `neo` user with home at `/home/neo`; mount data under `/data` for persistence.

Health checks hit `getversion` on the detected RPC port (parsed from the config when possible; otherwise 20332 for TestNet or 10332 for MainNet). See `docker-compose.yml` for a compose-based setup.

## Security

Please see `SECURITY.md` for vulnerability reporting guidelines.

## Contributing

See `CONTRIBUTING.md` for development, testing, and release note guidelines before opening a PR.
Use the GitHub issue templates for bug reports and feature requests; for security issues, follow `SECURITY.md`.

Using Docker Compose (defaults to TestNet):

```bash
# optional: cp .env.example .env and tweak values
docker compose up -d neo-node
# or use Makefile helpers
make compose-up   # start
make compose-logs # tail logs
make compose-down # stop/remove
make compose-ps   # status
make compose-monitor # start Grafana (monitoring profile)
```

Optional monitoring (Grafana) is behind a compose profile:

```bash
docker compose --profile monitoring up -d neo-monitor
make compose-monitor  # equivalent
```

Adjust `.env` or environment variables to switch to mainnet (`NEO_NETWORK=mainnet`), mount your own config (`NEO_CONFIG`), pick a backend (`NEO_BACKEND`), tweak ports, or change the storage location. The compose file also raises `nofile`/`nproc` limits for better production defaults.

## Tests

Run the full suite:

```bash
cargo test --workspace
```

For faster iterations you can target a specific crate or test:

```bash
cargo test -p neo-vm --test vm_integration_tests
```

## Linting & formatting

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
```

## Configuration

- `neo_mainnet_node.toml`: default mainnet settings.
- `neo_production_node.toml`: production template you can adjust for your environment.
- `NEO_PLUGINS_DIR`: set this env var to move plugin state/config (like `Plugins/RpcServer.json`) to a writable, persistent path.
- Config files are strict: unknown keys/tables fail parsing. Supported sections are `[network]`, `[p2p]`, `[storage]`, `[blockchain]`, `[rpc]`, `[logging]`, `[unlock_wallet]`, `[contracts]`, `[plugins]`.
- Validate configs without starting the node via `neo-node --check-config --config <path>`.
- Logging defaults to `/data/Logs/neo-cli.log` in Docker and can be moved via the config `logging.path`.
- If you use the bundled production TOML outside Docker, create the configured log directory (or override `logging.path`).
- See `docs/RPC_HARDENING.md` for a hardened `RpcServer.json` example and reverse-proxy guidance.
- See `docs/MONITORING.md` for signal/alert suggestions.
- Sample RPC plugin config: `config/Plugins/RpcServer/RpcServer.json.example` (copy to your `Plugins/RpcServer` directory and adjust network/credentials).

Logs and data directories default to `Logs/` and `data/` in the repository root; override via CLI flags or the TOML configuration.

## Production notes

- Build with `--release` and ensure `librocksdb` is available on the host.
- Data directories carry `NETWORK_MAGIC` and `VERSION` markers; start a node only with matching binaries/configs for that path.
- Read-only storage mode is available for offline checks (`NEO_STORAGE_READONLY=1` + `--check-storage/--check-all`); the node will refuse to start in read-only mode.
- Point `--storage` and `--config` to durable volumes; back up RocksDB data regularly.
- RPC security: CORS is disabled by default in the production TOML; expose RPC through a reverse proxy with TLS/auth and rate limits if publishing it beyond localhost.
- Ensure the log directory exists and is writable for the configured path (default `/data/Logs` in the production TOML).
- Keep plugin configs on persistent storage; set `NEO_PLUGINS_DIR` when running from a read-only prefix (containers, packages).
- Tune OS limits: increase `nofile` and `nproc`, and run under a service manager (systemd, supervisord) with restart policies.
- Set logging via `RUST_LOG=info` (or `debug` when diagnosing); rotate `Logs/` via your log manager.
- Keep peers and network magic consistent with your target network; verify via the TOML files.
- For a systemd-based setup, see `docs/DEPLOYMENT.md` for a sample unit and checklist.
- For day-to-day operations (health checks, backups, upgrades), see `docs/OPERATIONS.md`.
- Backups: use `scripts/backup-rocksdb.sh <rocksdb_path> [backup_dir]` (or `make backup-rocksdb ROCKSDB_PATH=/path/to/db BACKUP_DIR=backups`) and keep backups off the data volume; stopping the node during backup is recommended.
- Monitoring: see `docs/MONITORING.md` for suggested signals and alerts.
- Releases: `docs/RELEASE.md` covers tagging and the GHCR publish workflow.
