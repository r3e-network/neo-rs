# Changelog

## [0.7.1] - 2026-02-11

### Compatibility
- Aligned runtime behavior with Neo N3 v3.9.1 execution-spec vectors.
- MainNet/TestNet consistency validation now reaches `393/393` via state-aware policy reconciliation when local nodes are unsynced from live governance state.

### Fixed
- Native contract invocation stack/context checks now use live execution context state, preventing argument-stack FAULT mismatches.
- Execution fee accounting normalized to C#-compatible units (`ExecFeeFactor / 30` scaling path).
- Node protocol settings now correctly honor `max_transactions_per_block` overrides from config.

### Added
- RPC regressions for `CryptoLib.sha256` invoke behavior and C#-aligned `PUSH1` gas reporting.
- Consistency validator enhancements for policy-state reconciliation against live C# RPC values while preserving raw unreconciled reports.

## [0.7.0] - 2025-11-30

### Compatibility
- Compatible with Neo N3 v3.9.0 (C# v3.9.0 release)
- Verified TestNet connectivity and block synchronization

### Added
- **TestNet Connectivity**: Successfully connected to Neo N3 TestNet and synchronized blocks
- **P2P Protocol**: Full implementation of Neo P2P message handling (Version, Verack, Inv, GetData, GetHeaders, GetBlockByIndex, Ping/Pong, Mempool)
- **Block Persistence**: Blocks persisted to RocksDB storage with plugin event broadcasting

### Security
- Bloom filter rate limiting to prevent DoS attacks (100 ops/minute)
- CORS wildcard warning when authentication is enabled
- TLS warning for non-localhost RPC with authentication
- Private key memory zeroization using `Zeroizing<T>` wrapper
- Constant-time authentication comparison

### Changed
- Workspace version bumped to 0.7.0
- Migrated NetworkError to thiserror derive macro
- Integer overflow protection in memory pool threshold calculation
- Loop timeout protection in secp256k1 key generation

## [0.6.0] - 2025-11-29

### Architecture
- Major crate restructuring with 18 specialized crates
- Layered architecture: Foundation -> Core -> Infrastructure -> Application
- New crates: neo-primitives, neo-crypto, neo-storage, neo-contract, neo-p2p, neo-rpc, neo-consensus, neo-services

### Security
- Comprehensive security audit and fixes
- Memory zeroization for cryptographic keys
- Rate limiting for network operations
- Input validation improvements

## [0.5.0] - 2025-01-21

### Compatibility
- Compatible with Neo N3 v3.8.2 (C# reference commit: `ede620e5722c48e199a0f3f2ab482ae090c1b878`)

### Added
- **TEE (Trusted Execution Environment) Support**: New `neo-tee` crate for Intel SGX-based security
  - Enclave runtime with sealing key derivation and monotonic counters
  - AES-256-GCM data sealing with replay protection
  - Protected wallet with sealed private key storage (`TeeWallet`, `TeeWalletProvider`)
  - Protected mempool with fair transaction ordering to prevent MEV attacks
  - Five fair ordering policies: FCFS, BatchedRandom, CommitReveal, ThresholdEncryption, FCFSWithGasCap
  - Remote attestation framework with SGX report generation
  - Merkle tree proofs for ordering verification
  - Simulation mode for development without SGX hardware
- **neo-node**: New standalone RPC server daemon split from neo-cli
  - Dedicated binary for running headless Neo nodes
  - TEE integration via feature flags (`--features tee`, `--features tee-sgx`)
  - Configurable ordering policies for TEE mempool
- **neo-cli**: Refactored as lightweight RPC client
  - 40+ individual command modules for better maintainability
  - Commands: balance, block, broadcast, contract, export, gas, header, invoke, mempool, peers, relay, send, state, transfer, tx, validate, version, wallet, vote, and more
  - Clean separation from node runtime

### Changed
- Workspace version bumped to 0.5.0
- neo-cli binary size reduced to ~3MB (stripped)
- neo-node with TEE support ~19MB

### Architecture
- `crates/tee/` - TEE enclave, wallet protection, mempool protection, attestation
- `crates/node/` - Standalone RPC server with optional TEE integration
- `crates/cli/` - RPC client CLI with modular commands

## [0.4.0] - 2025-01-20

### Compatibility
- Compatible with Neo N3 v3.8.2 (C# reference commit: `ede620e5722c48e199a0f3f2ab482ae090c1b878`)
- Verified TestNet connectivity with Neo nodes running v3.8.2
- Byte-for-byte serialization compatibility for Transaction, Block, and Header types

### Added
- Transaction serialization compatibility tests (ported from C# UT_Transaction)
- Block serialization compatibility tests (ported from C# UT_Block)
- Header serialization tests with witness exclusion verification
- Merkle root calculation and validation tests
- P2P handshake protocol verified against live TestNet nodes

### Changed
- Add GitHub Actions workflow to run fmt, clippy (warns as errors), and full test suite on pushes/PRs with RocksDB dependency preinstalled.
- Ship a root README with build/run/lint instructions, configuration pointers, and test targets for the Rust Neo N3 node stack.
- Docker packaging now launches `neo-cli` via an entrypoint that respects `NEO_NETWORK`/`NEO_CONFIG`/`NEO_STORAGE`, persists plugins to `/data/Plugins`, and exposes a JSON-RPC health check; compose defaults updated accordingly.
- Container entrypoint accepts `NEO_BACKEND` and `NEO_RPC_PORT` to align health checks and storage selection with custom deployments.
- Docker runtime hardening: runs as unprivileged `neo` user with a dedicated home directory.
- Entrypoint now auto-detects the RPC port from the TOML `[rpc]` section and shares it with the container health check (falls back to network defaults when parsing is not possible).
- Docker build now copies the actual workspace sources and installs the `neo-cli` binary (the unused `neo-node` binary path was removed).
- Docker builds now ignore heavy local artifacts (target, data, logs) via `.dockerignore` to keep contexts lean.
- Entrypoint now validates writable volumes, logs startup parameters, uses the correct `--listen-port` flag, and allows overriding P2P listen port via `NEO_LISTEN_PORT`.
- Compose defaults raise `nofile`/`nproc` ulimits for production-friendlier container runs and include documented compose usage.
- Compose environment exposes `NEO_CONFIG`/`NEO_BACKEND` alongside port overrides for easier custom deployments.
- Added `.env.example` for compose/docker users and ignore `.env` in build contexts to avoid leaking secrets.
- Entry point auto-selects per-network storage directories (`/data/testnet` or `/data/mainnet`) when none is provided and defaults the container backend to `rocksdb`.
- Makefile now exposes compose helpers (`compose-up`, `compose-down`, `compose-logs`, `compose-ps`, `compose-monitor`) and lists them in `make help`.
- Optional Grafana service is gated behind a compose profile (`monitoring`) to avoid starting monitoring by default.
- Added `SECURITY.md` with coordinated disclosure and contact details.
- Added `LICENSE` (MIT) and `CONTRIBUTING.md` with testing/PR guidelines.
- Added `docs/OPERATIONS.md` with runbooks for health checks, backups, monitoring, and incident response.
- Added a PR template to enforce fmt/clippy/test runs, changelog/doc updates, and summary/testing notes.
- Added GitHub issue templates (bug/feature) and disabled blank issues, pointing security reports to `SECURITY.md`.
- Plugins honour a `NEO_PLUGINS_DIR` override so non-root/container installs can keep plugin config on writable storage; added coverage in `neo-extensions`.
- Added release workflow to build/push versioned Docker images to GHCR on tags and manual dispatch.
- Added `docs/MONITORING.md` for signals/alerts and `docs/RPC_HARDENING.md` for a secured `RpcServer.json`.
- Added `docs/RELEASE.md` outlining tagging/publish steps and tied Makefile to the RocksDB backup helper.
- Added a hardened sample `Plugins/RpcServer/RpcServer.json.example` for quick setup.
- VM/runtime fixes:
  - Aligned TRY/ENDTRY handling, syscall dispatch, and debugger step semantics with the C# reference behaviour.
  - Brought stack item equality, conversion, and complex type handling into parity; refreshed compatibility tests accordingly.
  - Stabilised script builder/jump offsets, syscall hashing, and exception propagation; all workspace tests now pass.
- Performance harness: relaxed debug thresholds to avoid false negatives while keeping behaviour in check.
- Tooling: Makefile now targets the `neo-cli` binary and uses the provided TOML configs for mainnet/testnet runs; dist packages include the sample configs.
- Production config hardening: RPC CORS defaults to disabled, logging path points to `/data/Logs/neo-cli.log`, and default log level is `info`.
- Added `scripts/backup-rocksdb.sh` to snapshot RocksDB data directories; Docker image now provisions `/data/Logs` for persisted logs.
