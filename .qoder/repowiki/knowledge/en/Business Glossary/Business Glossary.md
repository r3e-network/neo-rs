---
kind: business_term
name: Business Glossary
category: business_term
scope:
    - '**'
---

### StateRoot
- Definition：The Merkle root hash of Neo N3's MPT state tree for a given block height. The node can compute and validate it via the `--state-root` / `--stateroot` flags and the `NEO_STATE_ROOT` environment variable; enabling full historical state (`NEO_STATE_ROOT_FULL_STATE`) keeps old roots so proofs can be generated against prior heights.
- Aliases：state root、stateroot

### MainNet / TestNet
- Definition：Target networks selected by the `NEO_NETWORK` environment variable (or bundled TOML `neo_mainnet_node.toml` vs `neo_testnet_node.toml`). They differ in network magic, seed nodes, hardfork schedule, and persisted data directories under `data/mainnet` / `data/testnet`.
- Aliases：mainnet、testnet、network

### Hardened RPC
- Definition：A security preset for the JSON-RPC server that disables CORS, requires username/password authentication, and removes dangerous methods such as `openwallet` and `listplugins` by default. Activated via `--rpc-hardened` or corresponding CLI/env settings.
- Aliases：rpc-hardened、hardened mode

### TEE strict mode
- Definition：Mode that runs the node inside a Trusted Execution Environment (Intel SGX / TDX). Enabled with `--tee` plus `--tee-data-path`; the build itself requires the `tee` feature and a TEE-capable toolchain.
- Aliases：TEE、trusted execution environment

### HSM support
- Definition：Optional Hardware Security Module integration for signing operations, gated behind the `hsm` cargo feature and wired through the `neo-hsm` crate.
- Aliases：HSM、hardware security module

### Plugin system
- Definition：Neo N3's extensibility mechanism (ApplicationLogs, DBFTPlugin, RpcServer, TokensTracker) that was inlined into the Rust implementation but still configurable via a `Plugins/` directory and `NEO_PLUGINS_DIR` environment variable. Plugin configs (e.g. `RpcServer.json`) are written there at runtime.
- Aliases：plugin、Plugins、RpcServer plugin

### Backend (storage)
- Definition：The pluggable storage provider selected at startup. `rocksdb` is the default production backend (requires `--features full`); `memory` is the development-only in-memory backend. Selected via `--backend <memory|rocksdb>` or `NEO_BACKEND`.
- Aliases：storage backend、backend

### Read-only storage
- Definition：Startup mode that opens the RocksDB data directory without write access, used for offline checks and validation. Enabled by setting `NEO_STORAGE_READONLY=1` and running `--check-storage` / `--check-all`; the node refuses to start in this mode for normal operation.
- Aliases：read-only mode、NEO_STORAGE_READONLY
