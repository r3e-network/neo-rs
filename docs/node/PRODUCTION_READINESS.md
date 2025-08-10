### Overview

This document assesses the production readiness of the Neo-RS full node (Neo N3 in Rust) and defines the criteria and work needed to make it correct, complete, consistent, and production-ready.

- Scope: `node/` crate as the node process, with dependencies across `crates/` (network, consensus, ledger, vm, persistence, rpc_server, wallets, etc.)
- Goal: A node that can join MainNet/TestNet, fully sync, process transactions, expose a compliant JSON-RPC API, and (optionally) participate in dBFT consensus on a private network.

### Current Status Summary

- Node runtime
  - Consensus integration disabled in the entrypoint
    
```1:80:node/src/main.rs
// ... existing code ...
// mod consensus_integration; // Temporarily disabled due to compilation issues
// ... existing code ...
let consensus_service: Option<()> = if enable_consensus {
    warn!("🏛️  Consensus service temporarily disabled due to compilation issues");
    warn!("    The node will run in sync-only mode without consensus participation");
    None
} else {
    info!("⏭️  Consensus disabled");
    None
};
// ... existing code ...
```

  - RPC server wiring disabled
    
```630:651:node/src/main.rs
// use neo_rpc_server::RpcServer; // Temporarily disabled
// ... existing code ...
info!("⏭️ Skipping RPC server initialization for debugging/* implementation */;");
let rpc_server = Arc::new(());
```

- Networking
  - P2P stack present, but several TODOs in peer discovery and sync helpers
    
```1056:1061:crates/network/src/p2p_node.rs
// ... existing code ...
debug!("🔍 Running peer discovery");
// TODO: Implement peer discovery via GetAddr messages to connected peers
// ... existing code ...
```

  - Snapshot extraction not implemented
    
```699:710:crates/network/src/sync.rs
async fn extract_zstd_snapshot(&self, path: &str, height: u32) -> NetworkResult<()> {
    // TODO: Implement zstd extraction
}
async fn extract_gzip_snapshot(&self, path: &str, height: u32) -> NetworkResult<()> {
    // TODO: Implement gzip extraction
}
```

- RPC server
  - Exists in `crates/rpc_server`, routes implemented via `warp`, but node does not start it. Some methods have TODO placeholders (e.g., connection count).

- Consensus (dBFT)
  - Engine and service exist under `crates/consensus/` with state machine and events. The node does not instantiate or wire it yet.

- Persistence
  - RocksDB-based storage present (`neo-persistence` with RocksDB feature). Basic serialization helpers implemented. No critical gaps identified from scan, but integration validation is pending.

- Tests & CI hygiene
  - `cargo test --all` currently fails (notably `crates/mpt_trie` tests expecting APIs that differ from `neo-core`).
  - `cargo clippy -- -D warnings` fails with numerous errors in `neo-core` and other crates.
  - Formatting check passes.

### Production Readiness Checklist

- Node fundamentals
  - [ ] Clean startup/shutdown with signal handling and graceful component teardown
  - [ ] P2P peer management: discovery, handshake, inventory/headers/blocks exchange
  - [ ] Header and block sync end-to-end to latest BestHeight on MainNet/TestNet
  - [ ] Mempool and transaction relay consistent with protocol limits
  - [ ] JSON-RPC server enabled and compliant with Neo N3 API surface (core methods at minimum)
  - [ ] dBFT consensus available for PrivateNet and gated via feature/config

- Correctness & compatibility
  - [ ] Binary/protocol compatibility with C# Neo (headers, blocks, tx, VM semantics)
  - [ ] Genesis, native contracts, policy settings, and system fees aligned with network
  - [ ] Network magic, ports, seed lists correct for MainNet/TestNet

- Reliability & ops
  - [ ] Metrics/health endpoints (RPC `/health` exists; extend metrics surface)
  - [ ] Robust error handling with backoff/retries for networking/storage
  - [ ] Snapshots import/export implemented or explicitly documented as unsupported
  - [ ] Persistent data directory management, pruning (if any), backup strategy

- Security
  - [ ] Key material protection (wallets), NEP-2/NEP-6 conformance
  - [ ] No undefined `unsafe` usage in critical paths; audited where present
  - [ ] Input validation on all external boundaries (P2P, RPC)

- Code hygiene
  - [ ] `cargo build --all` clean
  - [ ] `cargo test --all` green (or flaked tests quarantined with justification)
  - [ ] `cargo clippy --all-targets` warnings triaged (allow rules where justified)
  - [ ] `cargo fmt` clean

### Identified Gaps (Blocking)

1. Consensus disabled in node entrypoint; cannot participate in dBFT
2. RPC server disabled in node entrypoint
3. P2P peer discovery and sync helpers have TODOs; snapshot extraction unimplemented
4. `cargo test --all` red due to `crates/mpt_trie` test expectations not matching current `neo-core` API (e.g., `UInt256::from_slice`, `hash().unwrap()`, `Option` semantics)
5. `cargo clippy -- -D warnings` red with numerous lints in `neo-core` and others

### Remediation Plan (Phased)

Phase 1: Make node runnable and observable
- Re-enable RPC in `node/src/main.rs` behind `--enable-rpc` / config flag
- Ensure basic RPC methods return real data (peer count, best height, peers list)
- Harden P2P startup: seed connections, handshake, ping/pong, version/headers flow verified on TestNet

Phase 2: Sync correctness
- Implement GetAddr-based peer discovery
- Validate headers/blocks pipeline against known MainNet hashes at checkpoints
- Implement snapshot extraction or document as unsupported; support gzip/zstd where possible

Phase 3: Consensus integration (PrivateNet)
- Wire `crates/consensus` to node with feature flag `consensus`
- Gate by config; ensure block production on PrivateNet works with default committee keys

Phase 4: Tests and lint hygiene
- Fix or update `crates/mpt_trie` tests to align with `neo-core` API
- Triage clippy errors: fix where simple; introduce crate-level `allow` for non-critical style lints to keep signal high
- Ensure `cargo test --all` and `clippy` green in CI

Phase 5: Documentation & ops
- Document configuration matrix (MainNet/TestNet/PrivateNet)
- Add operational runbooks and troubleshooting
- Define backup/restore and data directory layout

### Acceptance Criteria

- MainNet/TestNet: Node starts, establishes >8 peers, reaches latest header height, and remains synchronized for 24h.
- RPC: `getblockcount`, `getbestblockhash`, `getblock`, `getversion`, `getpeers`, `getconnectioncount`, `validateaddress` all return correct responses.
- PrivateNet with `--enable-consensus`: produces blocks at configured interval; transactions executed in VM.
- CI: `build`, `test`, `clippy` (with agreed allowlist) pass.

### Usage Examples

- Run a sync-only TestNet node with RPC:

```bash
cargo run -p neo-node -- --testnet --enable-rpc --rpc-bind 0.0.0.0 --rpc-port 10332
```

- Run a PrivateNet validator with consensus:

```bash
cargo run -p neo-node --features consensus -- --private --enable-consensus --enable-rpc
```

### Design Notes

- Keep consensus behind a feature flag to decouple consensus build from light/sync-only nodes.
- Use structured shutdown via `ShutdownCoordinator` to stop P2P, sync, RPC, and consensus in order.
- Prefer explicit allowlists for clippy at crate level to balance signal vs. churn.

### Test Coverage Targets

- P2P: handshake, version, ping/pong, inventory/headers/blocks message handling unit tests
- Sync: header chain validation vs. known checkpoints, block verification happy path
- RPC: per-method tests using an in-memory or temp-backed ledger
- Consensus: state transitions, timer handling, view change paths in PrivateNet mode

### Recent Behavior Updates (Rust Node)

- Seed node parsing: when hostname resolution is unavailable (tests/CI), entries like `host:port` fall back to `127.0.0.1:port` for deterministic parsing.
- Test-mode networking: when `NetworkConfig.port == 0`, P2P skips binding listeners/seed connections but still emits `NodeStarted` to support unit tests without sockets.
- Network message validation/compression: validation tuned to match Neo N3 semantics; compression heuristics avoid non-beneficial compression and tests may bypass it to keep payload sizes deterministic.
- Sync manager in tests: mirrors state/stats to enable deterministic assertions; background task handles are not required.
- RocksDB isolation in tests: blockchains created in tests use unique storage suffixes (UUID) to avoid RocksDB LOCK conflicts in concurrent tests.

Operational impact: no change to production defaults (non-zero ports, DNS available). These updates improve test determinism and portability.

