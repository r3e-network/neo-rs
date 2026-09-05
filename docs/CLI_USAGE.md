# neo-node Usage

Current command-line reference for the `neo-node` daemon and its JSON-RPC API in `neo-rs` v0.15.0 (Neo N3 v3.10.1).

> The former `neo-cli` client was merged into `neo-node`; use the node's JSON-RPC endpoint or an external RPC client.

## Quick Start

### Build binaries

```bash
# Node daemon (RPC client tools are external)
cargo build --release -p neo-node

# Optional TEE/SGX-enabled node binary
cargo build --release -p neo-node --features tee-sgx
```

### Start a node

```bash
# MainNet (default config path is neo_mainnet_node.toml)
./target/release/neo-node --config neo_mainnet_node.toml

# TestNet
./target/release/neo-node --config neo_testnet_node.toml

# Custom storage path + hardened RPC settings
NEO_RPC_USER=neo NEO_RPC_PASS='change-this' \
./target/release/neo-node \
  --config neo_mainnet_node.toml \
  --storage /var/lib/neo/mainnet \
  --rpc-hardened
```

Storage override note:
- `--storage <path>` now consistently overrides `storage.path` for startup checks and runtime store opening.
- Use an isolated `--storage` path for reproducible sync/validation runs.

### Preflight checks (without starting networking)

```bash
# Config schema/validation checks
./target/release/neo-node --config neo_mainnet_node.toml --check-config

# Storage backend access check
./target/release/neo-node --config neo_mainnet_node.toml --check-storage

# Both checks
./target/release/neo-node --config neo_mainnet_node.toml --check-all
```

## TEE Modes

`neo-node` supports three runtime modes:

```bash
# Strict TEE mode (fail-closed)
./target/release/neo-node \
  --config neo_mainnet_node.toml \
  --tee \
  --tee-data-path ./tee_data

# Opportunistic TEE mode (fallback to ordinary mode)
./target/release/neo-node \
  --config neo_mainnet_node.toml \
  --tee-auto \
  --tee-data-path ./tee_data

# Ordinary mode (no TEE)
./target/release/neo-node --config neo_mainnet_node.toml
```

Notes:
- `--tee` is strict. TEE init/self-check/attestation failures stop startup.
- `--tee-auto` attempts TEE first; on failure it logs a warning and continues without TEE.
- If `--tee*` flags are missing from `neo-node --help`, rebuild with `--features tee` or `--features tee-sgx`.
- For full SGX runtime validation (peer connectivity, sync progression, repeated RPC checks, TEE wallet export denial), use:
  `scripts/validate-tee-sgx-runtime.sh --iterations 100 --require-block-progress`.
- If default ports are already used by another local process, run validator with explicit overrides:
  `--listen-port <p2p_port> --rpc-port <rpc_port> --rpc-url http://127.0.0.1:<rpc_port>`.
- If DCAP returns non-terminal QV status (for example `0xA008`), strict mode fails closed by default; use `--allow-non-terminal-qv` only as an explicit operator override.

## neo-node Reference

Usage:

```bash
neo-node [OPTIONS]
```

Key options:

| Category | Options |
|----------|---------|
| Config & storage | `--config`, `--storage`, `--backend`, `--storage-read-only` |
| Network | `--network-magic`, `--listen-port`, `--seed`, `--max-connections`, `--min-connections`, `--max-connections-per-address`, `--broadcast-history-limit`, `--disable-compression`, `--block-time` |
| RPC | `--rpc-bind`, `--rpc-port`, `--rpc-disable-cors`, `--rpc-user`, `--rpc-pass`, `--rpc-tls-cert`, `--rpc-tls-cert-password`, `--rpc-allow-origins`, `--rpc-disabled-methods`, `--rpc-hardened` |
| Logging | `--logging-path`, `--logging-level`, `--logging-format`, `--daemon` |
| Health & sync | `--health-port`, `--health-max-header-lag` |
| Import | `--import-acc`, `--import-only` |
| Validation checks | `--check-config`, `--check-storage`, `--check-all` |
| State root | `--state-root`, `--state-root-path`, `--state-root-full-state` |
| Wallet | `--wallet`, `--wallet-password` |
| TEE (feature-gated) | `--tee`, `--tee-auto`, `--tee-data-path`, `--tee-ordering-policy` |

Environment variables:
- Most options can also be set via env vars (`NEO_CONFIG`, `NEO_STORAGE`, `NEO_RPC_PORT`, `NEO_LOG_LEVEL`, etc.).
- Run `neo-node --help` to see the exact env var attached to each flag.
- During `--import-acc`, `neo-node` auto-selects `NEO_ROCKSDB_BATCH_PROFILE=high_throughput` unless you set `NEO_ROCKSDB_BATCH_PROFILE` explicitly.

## Interacting with a running node

`neo-node` is a daemon that exposes its capabilities through flags (for
startup/validation) and a JSON-RPC API (for querying chain, wallet, and
invoking contracts). There is no separate `neo-cli` client binary — use the
node's JSON-RPC endpoint or an external RPC client.

### Startup & validation flags

```bash
# Config/storage checks without starting networking
neo-node --config neo_mainnet_node.toml --check-config
neo-node --config neo_mainnet_node.toml --check-storage
neo-node --config neo_mainnet_node.toml --check-all
```

### JSON-RPC queries

```bash
# Node and chain state
curl -s http://localhost:10332 -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"getblockcount","params":[]}'

curl -s http://localhost:10332 -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"getblock","params":[1000,1]}'

curl -s http://localhost:10332 -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"getrawtransaction","params":["0x<tx_hash>",1]}'

# Contract invocation (read-only)
curl -s http://localhost:10332 -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"invokefunction","params":["0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5","totalSupply",[]]}'

# Peers / mempool
curl -s http://localhost:10332 -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"getpeers","params":[]}'
```

Run `neo-node --help` for the full flag list and `--rpc-port`/`NEO_RPC_PORT`
for the JSON-RPC endpoint configuration.

## Troubleshooting

```bash
# Show all available node flags (depends on enabled features)
neo-node --help
```
