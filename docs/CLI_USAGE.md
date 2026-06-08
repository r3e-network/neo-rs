# neo-node and neo-cli Usage

Current command-line reference for `neo-node` (daemon) and `neo-cli` (JSON-RPC client) in `neo-rs` v0.7.x.

## Quick Start

### Build binaries

```bash
# Standard node + client
cargo build --release -p neo-node -p neo-cli

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

## neo-cli Reference

`neo-cli` is an RPC client. It does not run networking/P2P; it talks to an RPC endpoint.

Usage:

```bash
neo-cli [OPTIONS] <COMMAND>
```

Global options:
- `-u, --rpc-url <RPC_URL>` (default: `http://localhost:10332`)
- `--rpc-user <RPC_USER>`
- `--rpc-pass <RPC_PASS>`
- `-o, --output <OUTPUT>` where `OUTPUT` is `plain|table|json`

Main commands:
- `version`
- `state`
- `peers`
- `mempool`
- `plugins`
- `start-consensus`
- `block`
- `header`
- `tx`
- `contract`
- `best-block-hash`
- `block-count`
- `block-hash`
- `balance`
- `transfers`
- `gas`
- `invoke`
- `test-invoke`
- `wallet`
- `send`
- `transfer`
- `vote`
- `unvote`
- `register-candidate`
- `unregister-candidate`
- `candidates`
- `committee`
- `validators`
- `native-contracts`
- `neo`
- `gas-token`
- `parse`
- `parse-script`
- `validate-address`
- `sign`
- `relay`
- `broadcast`
- `export-blocks`

## Common neo-cli Examples

```bash
# Node and sync state
neo-cli state
neo-cli peers
neo-cli block-count

# Chain data
neo-cli block 1000
neo-cli block 0x<block_hash> --raw
neo-cli tx 0x<tx_hash>

# Contract invocation (read-only)
neo-cli invoke 0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5 totalSupply '[]'

# Token and account info
neo-cli balance NQy6...address...
neo-cli transfers NQy6...address...
neo-cli gas NQy6...address...

# Wallet subcommands
neo-cli wallet create ./wallet.json
neo-cli wallet open ./wallet.json
neo-cli wallet list

# Output control
neo-cli --output json state
neo-cli --rpc-url http://127.0.0.1:20332 state
```

## Troubleshooting

```bash
# Show all available node flags (depends on enabled features)
neo-node --help

# Show client command tree
neo-cli --help

# Show command-specific help
neo-cli invoke --help
neo-cli wallet --help
```
