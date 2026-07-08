# Changelog

## [Unreleased]

### Changed
- **Neo N3 v3.10.1 compatibility target.** Added `HF_Huyao` to the canonical hardfork enum and updated the built-in protocol compatibility documentation. MainNet/TestNet presets continue to schedule hardforks through `HF_Faun`; `HF_Gorgon` and `HF_Huyao` are defined but unscheduled unless explicitly configured.

### Fixed (Neo N3 v3.10.1 consensus / protocol parity)
- **ApplicationEngine fee validation.** Negative `AddFee` inputs now fault before the whitelist fee bypass, matching C# v3.10.1 ordering and preventing a whitelisted call context from silently ignoring an invalid negative fee.

## [0.10.0] - 2026-07-03

### Added
- **Signed StateRoot subsystem (StateValidators).** Completes the C# `StateService` surface: `neo_blockchain::verify_state_root` (witness verification against the StateValidators BFT multisig at 2 GAS), the vote sign/validate/`M`-of-`N` aggregation core + collector, and a node-level P2P consensus driver (`neo-node/src/state_root`) that subscribes to block-persist, signs and relays `Vote` messages over the `"StateService"` extensible category, aggregates and verifies the signed root, persists it, and rotates the relay sender. `StateRoot` is now `IVerifiable` (witness field, `GetSignData(network)`, 0/1-witness var-array serialization) and `getstateroot` returns witnesses. New `[state_service].validator_key_hex` config (absent = observer that verifies + persists inbound signed roots).

### Fixed (Neo N3 v3.10.0 consensus / protocol parity)
- **P2P handshake compatibility (critical).** `VersionPayload` no longer serializes a phantom top-level `StartHeight`; C# writes only `Network|Version|Timestamp|Nonce|UserAgent|Capabilities`, with the height carried inside the `FullNode` capability. The extra 4 bytes misaligned the capability var-int and made every `version` message mutually unparseable with real Neo nodes — the node could not complete the handshake against mainnet/testnet.
- **`CryptoLib.recoverSecp256K1` (corrects 0.9.0).** Now accepts both 65-byte and 64-byte (EIP-2098 compact) signatures. C# `Crypto.ECRecover` accepts both (`if (len != 65 && len != 64) throw`); the 0.9.0 "65-only" restriction was based on a misreading and diverged on the HF_Echidna-active path.
- **Oracle HTTPS redirects (corrects 0.9.0).** A redirect is followed whenever a `Location` header is present, regardless of status (matching C# `OracleHttpsProtocol`); the 0.9.0 "3xx-only" gate was wrong. SSRF filtering now blocks a host if **any** resolved DNS address is internal (C# `IPHostEntry.IsInternal` = `AddressList.Any`).
- **`CryptoLib.verifyWithECDsa`.** A malformed public key now faults (C# `ECPoint.DecodePoint` throws `FormatException`, which is not caught by `catch(ArgumentException)`); the key is decoded before the signature-length check, matching C# ordering.
- **`CryptoLib.bls12381Equal`.** A cross-group comparison (e.g. G1 vs G2) now faults (`ArgumentException("BLS12-381 type mismatch")`) instead of returning false.
- **VM `SHL`/`SHR`.** Neo.VM 3.10.0 removed the zero-shift early-return: the value operand is always `GetInteger()`-coerced, so a zero shift faults on a `Buffer`/`Null` and re-pushes a `Boolean`/`ByteString` as `Integer`.
- **dBFT commit gate.** `check_prepare_responses` now requires `RequestSentOrReceived` (the PrepareRequest was sent or received) before signing a `Commit`, so `M` PrepareResponses arriving before the PrepareRequest can no longer commit-sign the default (zero) block hash.
- **dBFT wire/behavior.** `ChangeView` serializes only `Timestamp+Reason` (removed a non-C# `RejectedHashes` array); added the view-backward guard (`CheckExpectedView`) and the own-`ChangeView(ChangeAgreement)` broadcast on reaching `M`; a PrepareResponse from the primary index no longer double-counts.
- **`StdLib.jsonSerialize` / `jsonDeserialize`.** Invalid-UTF-8 `ByteString`/`Buffer` values fault via strict `from_utf8` (C# `StrictUTF8`); integers `> 2^53` route through `f64` to match C#'s lossy double.
- **`StdLib` int parameters.** `atoi`/`itoa` `base` and `memorySearch` `start` now wrap via a `.NET` `(int)` truncating cast instead of faulting on out-of-`i32` values.
- **`ContractManagement.destroy`.** Added the HF_Gorgon block-before-erase ordering (dormant on v3.10.0 mainnet, which does not schedule Gorgon).
- **`DataCache` merge.** The fast-path merge now store-checks a `Deleted` tombstone for a store-absent key (matching C# `DataCache.Delete`), keeping a spurious tombstone out of the MPT state-root change set.
- **RPC:** `invokefunction`/`invokescript` emit `"interface":"IIterator"` (was `"StorageIterator"`); a `NotYetValid` relay result maps to `ExpiredTransaction` (`-510`).
- **NEP-2:** wallet key encryption/decryption threads the NEP-6 wallet's own `ScryptParameters` instead of hardcoding `16384/8/8`.
- **neo-system oracle admission** fails closed on ledger read errors.

### Changed
- Migrated the workspace to the `neo-vm-rs` 0.2.0 `StackValue` API (reference-id compound equality; id excluded from serialization).
- Fixed a latent build break in `neo-storage` (`mdbx/snapshot.rs` used `error!` without importing it).

## [0.9.0] - 2026-07-03

### Architecture
- Added supervised daemon task handling in `neo-node`, with essential task failures requesting graceful shutdown and normal task failures reported through bounded observability labels.
- Added `neo_runtime::BlockImportQueue` for bounded concurrent block preverification before ordered canonical import.
- Added typed storage table adapters in `neo-storage` and provider factories for hot/cold ledger and immutable state reads.
- Documented the updated architecture, dataflow, and coding guidance for task supervision, import queues, table codecs, and provider factories.

### Fixed (Neo N3 v3.10.0 consensus / protocol parity)
- **dBFT commit gate:** a backup no longer signs a `Commit` until it holds every proposed transaction. `check_prepare_responses` now applies the `has_missing_proposed_transactions()` gate that `check_commits` already had, matching C# `ConsensusService.CheckPreparations` (`PreparationPayloads >= M && TransactionHashes.All(ContainsKey)`).
- **Stale consensus snapshot:** the consensus driver now takes a fresh store snapshot at the start of every round (`ConsensusDriver::fresh_round_snapshot`), so committee / validator / `NextConsensus` reads reflect the current tip across committee-refresh heights — matching C# `ConsensusContext.Reset` — instead of reusing a frozen startup snapshot.
- **`StdLib.jsonSerialize` UTF-8:** invalid-UTF-8 `ByteString`/`Buffer` values now fault via strict `from_utf8` instead of silently substituting `U+FFFD`, matching C# `StrictUTF8` (removes a VM-reachable execution divergence).
- **`CryptoLib.recoverSecp256K1`:** rejects any signature that is not exactly 65 bytes at the consensus boundary, matching C# `Crypto.ECRecover`. Previously a 64-byte EIP-2098 signature recovered a key here but returned `null` on C# (Echidna, live mainnet). Adds a parity regression test.
- **HSM CheckSig hash:** the PKCS#11 single-signature redeem script now derives the `System.Crypto.CheckSig` interop hash from `sha256(name)[..4]` (`0x27b3e756`) instead of a wrong hardcoded value, fixing the derived HSM validator identity.
- **Oracle HTTPS:** redirects are followed only on 3xx responses (matching C#); removed a per-chunk 8 KB cap that could wrongly reject legitimate sub-64 KB responses delivered in one stream frame.
- **Wallet witness:** `create_witness` now uses the shared `signature_invocation` helper, validating signature length before the `PUSHDATA1` length byte (no silent `len as u8` truncation).
- Minor: checked decimal addition in `BigDecimal` multiplication; corrected oracle minimum-gas error text; corrected the JSON quote-escape doc comment.

### Changed
- Migrated the workspace to the **`neo-vm-rs` 0.2.0** `StackValue` API: the compound variants (`Buffer`/`Array`/`Struct`/`Map`) now carry a leading reference-identity `u64` id, and `next_stack_item_id()` returns `u64`. The id is never serialized, so binary/JSON wire formats and consensus hashes are unchanged. Equality of compound stack values is now reference-based (C# `ReferenceEquals` semantics).

### Internal / tests
- Converted test assertions that compared compound `StackValue`s by value to a structural helper (0.2.0 compares compounds by reference id).
- Fixed a parallel test-isolation flake in `neo-system`: a shared, poison-resilient guard now serializes every test that touches the process-global native-contract provider.
- Architecture tests: classified `neo-static-files` in the layer map and corrected stale file paths in the retired-term doc checks.

## [0.8.0] - 2026-06-14

### Added
- `neo-hsm` crate: a multi-cloud PKCS#11 `ConsensusSigner` (AWS CloudHSM / Azure Dedicated+Cloud HSM / GCP) implementing the `neo-consensus` signer seam, with optional native Azure/GCP REST backends and no `unsafe`.
- Cross-implementation benchmark harness (`benchmarks/`): a standalone JSON-RPC load generator plus offline `.acc` block-import throughput, RPC, and resource-sampling scripts for comparing neo-rs against neo-cli (C#) and neo-go.
- Native node GUI / manager (`neo-gui`) — an eframe/egui application for configuring, running, and monitoring the node.

### Changed
- Uniform struct-based API: every native-contract operation, plus the genuine local-type operations across other crates (storage, execution, payloads, rpc), are now methods/associated functions on their owning struct instead of module-level free functions.
- Removed the upstream C# Neo MIT-license headers that had been copied into Rust source files (they cover the C# project's files, not this Rust node); genuine module documentation is preserved.
- `.gitignore` now excludes AI assistant tooling artifacts (`.agents/`, `.claude/`, etc.).

### Fixed
- HSM public-key decode now disambiguates a bare X9.62 EC point (which begins with `0x04`, identical to the DER OCTET STRING tag) from a DER-wrapped one by length, preventing a wrong consensus script hash.

### Breaking changes (workspace restructuring)

The `neo-core` monolith has been split into four focused, single-responsibility crates. The end state mirrors the polkadot-sdk / reth convention: a thin `neo-core` compatibility facade over a layered workspace where each crate owns exactly one domain.

**New crates (Layer 0 — Foundation):**
- `neo-error` — authoritative `CoreError` / `CoreResult` for the whole workspace. Replaces the duplicate error types that previously lived inside `neo-core`.
- `neo-time` — testable `TimeProvider` / `TimeSource`. Replaces `neo_core::time_provider::*`.

**New crates (Layer 1 — Protocol):**
- `neo-ledger-types` — pure ledger / wire data types. Owns `Witness`. The canonical home for `Block` / `Header` / `Transaction` / `Signer` in future slices.

**New crates (Layer 2 — Service):**
- `neo-chain` — pure block / chain validation (`BlockValidator`, `BlockValidationError`, `validate_merkle_root`, `validate_witness_scripts`, etc.). Has **zero** dependency on `neo-core`; the public API now takes `&[UInt256]` hashes and `&Witness` references rather than concrete `Header` / `Transaction` types.

**Migration table:**

| Old import path | New import path |
|-----------------|-----------------|
| `neo_core::CoreError` / `neo_core::error::CoreError` | `neo_error::CoreError` |
| `neo_core::CoreResult` / `neo_core::error::CoreResult` | `neo_error::CoreResult` |
| `neo_core::Result` | `neo_error::Result` |
| `neo_core::TimeProvider` | `neo_time::TimeProvider` |
| `neo_core::time_provider::*` | `neo_time::*` |
| `neo_core::Witness` | `neo_ledger_types::Witness` |
| `neo_core::witness::*` | `neo_ledger_types::witness::*` |
| `neo_core::validation::*` / `BlockValidator` | `neo_chain::block_validation::*` |

The old import paths are still re-exported from `neo_core::*` for one release cycle to give downstream callers a graceful migration window. New code should import from the canonical crates.

### Internal cleanups

- Fixed pre-existing macro bug: `impl_native_contract!` and `neo_native_contract_methods!` in `neo-core` referenced an unresolved `$neo_error::` placeholder left over from a half-finished extraction. Now correctly emits `::neo_error::` paths.
- Moved the orphan-rule-violating `impl From<KeyBuilderError> for CoreError` out of `neo-core` and into `neo-error` (where it belongs, since `CoreError` lives there).
- Centralized all `From<X> for CoreError` impls in `neo-error` (was previously split between `neo-core::error` and individual consumers). Documented as a TODO to move each `From` into the source crate once those crates are independently versioned.
- Removed the now-duplicate `error.rs`, `time_provider.rs`, `witness.rs`, `validation.rs` source files from `neo-core/src/`. Replaced `pub mod xxx;` declarations with `pub use neo_xxx::*;` re-exports for backward compatibility.
- Bulk-migrated 89 internal `crate::error::*` / `crate::time_provider::*` / `crate::witness::*` / `crate::validation::*` references in `neo-core` to the new home crates via sed.
- Updated 42 external `neo_core::Witness` / `neo_core::error::*` / `neo_core::time_provider::*` references across the workspace (`neo-rpc`, `neo-consensus`, `neo-p2p`, `neo-node`, `neo-tx-builder`, integration tests).

### Verification

- `cargo check --workspace` — **green** (0 errors).
- `cargo test --workspace --lib` — **2048 passed, 0 failed, 8 ignored** across 27 test suites.
- `neo-error` lib + doc tests: 7 unit + 1 doc — all green.
- `neo-time` lib + doc tests: 1 unit + 1 doc — all green.
- `neo-ledger-types` lib + doc tests: 8 unit + 1 doc — all green.
- `neo-chain` lib tests: 22 unit — all green.

## [0.7.2] - 2026-02-12

### Fixed
- RPC getnativecontracts now returns active native contracts even when persisted native contract states are not yet present in local storage.
- Compatibility with execution-spec native/crypto vector lanes restored for cold-start nodes.

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
