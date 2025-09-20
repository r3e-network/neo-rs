# Consistency Review Checklist

Legend: [ ] To do • [~] In progress • [x] Done

Parity target: Behavior matches C# Neo N3; no API drift.

How to use:
- Work top to bottom. Build after each section. Check off items.
- Build quickly: `cargo build --workspace -q`
- Search code: `rg "<symbol or type>"` • List files: `rg --files`

Order of work:
1) Extensions → 2) Plugin System → 3) RPC Plugin → 4) DBFT Plugin → 5) RPC Server crate → 6) Consensus → 7) Ledger → 8) Networking → 9) Mempool → 10) Storage → 11) CLI/Config → 12) Tests/Docs

Tackle first
- [x] rpc_server params: unify `Option<Value>` vs `Value` across `lib.rs` and `methods.rs`.
- [x] DbftPlugin config: map to `ConsensusConfig` (`validator_count`, `block_time_ms`, `view_timeout_ms`, etc.).

Extensions (crates/extensions)
- [ ] PluginContext shape: `neo_version`, `config_dir`, `data_dir`, `shared_data` only.
- [ ] Config access via `context.config_dir` (file-based); remove any `context.config` usage.
- [ ] Events: `PluginEvent::{NodeStarted, NodeStopping, BlockReceived{block_hash, block_height}, TransactionReceived{tx_hash}}` used consistently.
- [ ] Errors: use `ExtensionError::invalid_config` for config problems; map others appropriately.
- [ ] Docs/comments: public examples reflect current API.

Done when: All crates compile against a single `PluginContext` and `PluginEvent` definition; no stale fields.

Plugin System (crates/plugins)
- [ ] `PluginLoader::base_directory()` accessor exists and is used; no private field peeks.
- [ ] Construct `PluginContext` with neo version + `config_dir` + `data_dir` from base dir.
- [ ] Ensure dirs exist; no panics if missing.
- [ ] Lifecycle ordering deterministic; emits `NodeStarted` and `NodeStopping` to all plugins.
- [ ] Logging: consistent target/module names.

Files: `crates/plugins/src/plugin_loader.rs`, `crates/extensions/src/plugin.rs`, any `complete_plugin_system.rs`.

RPC Server Plugin (crates/plugins/src/rpc_server.rs)
- [x] `use warp::Filter;` present; filters compose without trait errors.
- [ ] Shared methods stored as `Arc<RwLock<HashMap<...>>>` for filter capture.
- [ ] No `&self` captured in async closures; use helper (e.g., `handle_rpc_request_with_methods`).
- [ ] Handles `PluginEvent::BlockReceived`/`TransactionReceived` as intended.
- [ ] Loads `RpcServer.json` from `config_dir` with sane defaults; config errors map to `invalid_config`.
- [ ] Graceful shutdown on `NodeStopping`.

Done when: Plugin starts, serves, and stops cleanly; no lifetime/borrow issues.

DBFT Plugin (crates/plugins/src/dbft_plugin.rs)
- [x] Load `DBFTPlugin.json` from `config_dir` (file-based only).
- [x] Map to `ConsensusConfig` fields: `validator_count`, `block_time_ms`, `view_timeout_ms`, `max_view_changes`, `enable_recovery`, `recovery_timeout_ms`, `max_block_size`, `max_transactions_per_block`, `enable_metrics`.
- [x] Remove/ignore deprecated fields: `network_type`, `enabled`, `view_change_timeout`, `min_committee_size`.
- [ ] Invalid/missing fields → `ExtensionError::invalid_config`.
- [ ] No hard dependency on ledger; expose hooks for later injection.

Done when: Compiles and initializes from file-only config; no unknown fields used.

RPC Server crate (crates/rpc_server)
- [x] Choose consistent param type (recommend `Option<Value>`); align all of `methods.rs` and `methods_extended.rs`.
- [x] Update `lib.rs` call sites to pass/unwrap consistently (e.g., `req.params.unwrap_or(Value::Null)`).
- [ ] `types.rs` matches JSON-RPC 2.0 shapes (id, error codes).
- [ ] Structured JSON-RPC errors; no panics on bad input.
- [ ] Add/fix tests for representative methods.

Consensus (crates/consensus)
- [ ] Define `ConsensusService` API based on `ConsensusConfig`.
- [ ] DBFT plugin starts/stops consensus via service.
- [ ] Respect timing: `block_time_ms`, `view_timeout_ms`.
- [ ] Emits events: `BlockReceived` when a block is accepted.

Done when: Consensus can be started/stopped via plugin without runtime errors.

Ledger (crates/ledger)
- [ ] Service trait exposes read/write used by RPC/consensus.
- [ ] Storage uses `data_dir` for DB paths.
- [ ] Hashing/serialization match N3 formats.
- [ ] RPC surfaces: block headers, balances, state root available.

Done when: Minimal ledger backs RPC reads and accepts committed blocks.

Networking (crates/network)
- [ ] Peer management: connections, handshakes, N3 version compatibility.
- [ ] Config: listen/seed addresses loaded from file.
- [ ] Relay: tx/block relay into mempool/ledger hooks.

Done when: Node connects and relays basic messages in a testnet-like environment.

Mempool (likely crates/ledger or dedicated)
- [ ] Validation: signatures, fee policy, duplicates, simple policy checks.
- [ ] Emits `TransactionReceived` events.
- [ ] RPC integration: `sendrawtransaction`, `getrawmempool` read from mempool.

Done when: RPC can submit and list mempool transactions.

Storage (crates/persistence, crates/ledger)
- [ ] DB paths derived from `data_dir` consistently.
- [ ] Schema: column families/tables defined for ledger state.
- [ ] Maintenance: compaction/pruning toggles (if applicable).

Done when: Storage is stable across restarts.

CLI/Config (crates/cli, crates/config)
- [ ] Global config path strategy; pass dirs into `PluginContext`.
- [ ] Document schemas for `RpcServer.json`, `DBFTPlugin.json` (expected fields, defaults).
- [ ] Include sample configs for quick start.

Parity with C# Neo N3
- [ ] RPC parity: method names, params, and responses match N3 where implemented.
- [ ] Consensus defaults match N3.
- [ ] Semantics: event timing and plugin lifecycle mirror N3 behavior.

Build and validate
- [ ] Workspace build: `cargo build --workspace`
- [ ] Targeted builds: `cargo build -p extensions -p plugins -p rpc_server`
- [ ] Tests: `cargo test -p rpc_server` (+ others if present)
- [ ] Smoke run: start plugin system; hit a couple RPCs.

Documentation (top-level and crate READMEs)
- [ ] READMEs reflect `PluginContext` and config dirs.
- [ ] Migration notes: call out API/signature changes.
- [ ] Minimal example config to run RPC server.

Notes and status
- Use this section to jot down blockers, decisions, and cross-links to issues.
