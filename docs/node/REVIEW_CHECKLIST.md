### Neo-RS Full Node Review Checklist

Use this checklist when reviewing changes to ensure the node remains correct, complete, consistent, and production-ready.

#### Architecture & Wiring
- [ ] Entry point (`node/src/main.rs`) initializes: storage, ledger, P2P, sync, mempool, RPC (if enabled), consensus (if enabled)
- [ ] `ShutdownCoordinator` stops components in correct order
- [ ] Feature flags and CLI/config control RPC and consensus

#### Networking
- [ ] P2P handshake and version messages handled according to Neo N3
- [ ] Peer discovery via seeds and GetAddr implemented
- [ ] Inventory/headers/blocks flow validated
- [ ] Backpressure and message buffer limits enforced
- [ ] Timeouts, retries, ban logic (if applicable)

#### Sync & Ledger
- [ ] Header-first sync pipeline correct
- [ ] Block verification adheres to limits (size, tx count)
- [ ] Mempool limits and eviction policies configured
- [ ] Snapshot import/export (or documented unsupported)

#### Consensus (when enabled)
- [ ] dBFT service wired to networking
- [ ] Timers, view change, and state transitions covered by tests
- [ ] PrivateNet block production verified

#### RPC
- [ ] RPC server starts when enabled
- [ ] Core methods return accurate data (block count, best hash, peers)
- [ ] Input validation; errors map to JSON-RPC codes
- [ ] Health endpoint available

#### Persistence
- [ ] RocksDB options sane for production (compaction, compression)
- [ ] Data directory layout documented and configurable
- [ ] Backups (and restore) documented

#### Security & Safety
- [ ] No unchecked `unsafe` in critical paths; audited where present
- [ ] Wallet operations (NEP-2/NEP-6) correct; secrets never logged
- [ ] External boundary inputs validated (P2P, RPC)

#### Quality Gates
- [ ] `cargo build --all` passes
- [ ] `cargo test --all` passes or quarantined with justification
- [ ] `cargo clippy -- -D warnings` passes with allowlists justified
- [ ] `cargo fmt -- --check` passes

#### Documentation
- [ ] `docs/node/PRODUCTION_READINESS.md` updated to reflect changes
- [ ] Usage examples runnable
- [ ] Configuration options documented (MainNet/TestNet/PrivateNet)


