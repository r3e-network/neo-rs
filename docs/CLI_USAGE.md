# neo-node and JSON-RPC Usage

Current command-line reference for `neo-node` in `neo-rs` v0.7.x.

## Quick Start

### Build binaries

```bash
# Standard node daemon
cargo build --release -p neo-node
```

### Start a node

```bash
# MainNet
./target/release/neo-node --config neo_mainnet_node.toml

# TestNet
./target/release/neo-node --config neo_testnet_node.toml

# Custom storage path
./target/release/neo-node \
  --config neo_mainnet_node.toml \
  --storage-path /var/lib/neo/mainnet
```

Storage override note:
- `--storage-path <path>` overrides `[storage].data_dir` / `[storage].path` and implies RocksDB.
- Use an isolated `--storage-path` for reproducible sync/validation runs.

### Preflight checks (without starting networking)

```bash
# Config schema/validation checks
./target/release/neo-node --config neo_mainnet_node.toml --check-config

# Storage backend access check
./target/release/neo-node --config neo_mainnet_node.toml --check-storage

# Both checks
./target/release/neo-node --config neo_mainnet_node.toml --check-all
```

## neo-node Reference

Usage:

```bash
neo-node [OPTIONS]
```

Options:

- `-c, --config <CONFIG>`: TOML config path. Default: `neo_testnet_node.toml`.
- `--network-magic <NETWORK_MAGIC>`: override the protocol network magic.
- `--storage-path <STORAGE_PATH>`: override the RocksDB directory.
- `--check-config`: validate parsed configuration and exit.
- `--check-storage`: validate the configured storage backend can be opened and exit.
- `--check-all`: run all preflight checks and exit.

Other node settings are TOML-driven. Edit `[storage]`, `[p2p]`, `[rpc]`,
`[blockchain]`, `[mempool]`, and `[consensus]` sections instead of passing
runtime flags for those knobs. Use `RUST_LOG` for tracing filters.

## JSON-RPC Reference

Enable `[rpc] enabled = true` in the TOML, then query the node over HTTP:

```bash
curl -s --compressed -X POST http://127.0.0.1:10332 \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"getversion","params":[]}'
```
