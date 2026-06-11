# Stage C - `neo-network` P2P host services

> Status: **in progress (foundation landed; full state-machine port deferred)**

This change replaces the three Akka-style actor modules
(`local_node`, `remote_node`, `task_manager`) currently living in
`neo-core/src/network/p2p/` with reth-style services in
`neo-network/src/`. The target pattern matches Stage B
(`neo-blockchain`): a single `async fn run(self)` command loop driven
by a `tokio::sync::mpsc::Receiver<Command>`, a `tokio::sync::broadcast::Sender<Event>`,
and a `CancellationToken` for shutdown.

The trait-level contract is already in place at
`neo_runtime::NetworkService` (Stage A). This change implements that
trait and adds the three concrete services.

## Scope boundaries

The actor code under `neo-core/src/network/p2p/` is **not** deleted
in this change. It is left in place (still gated behind
`#[cfg(feature = "runtime")]`) so existing consumers keep compiling.
The new services in `neo-network` are *additive*; the migration of
consumers off the actor handles is a Stage E task.

## Files

```
neo-network/
├── Cargo.toml                 — adds tokio/async-trait/futures/tokio-util/
│                                 thiserror/tracing, neo-runtime, neo-payloads,
│                                 neo-ledger-types, neo-primitives, neo-config,
│                                 neo-blockchain, neo-mempool, neo-core (legacy
│                                 re-export), neo-actors, neo-services,
│                                 neo-events, neo-state-types, neo-p2p
├── tests/integration.rs       — 5 smoke tests for the new services
└── src/
    ├── lib.rs                 — module exports + crate-level doc
    ├── error.rs               — NetworkError (thiserror)
    ├── event.rs               — re-export of `neo_runtime::NetworkEvent`
    ├── command.rs             — NetworkCommand (top-level)
    ├── handle.rs              — NetworkHandle (mpsc::Sender + broadcast::Sender)
    ├── peer_id.rs             — PeerId (64-bit unique id)
    ├── local_node.rs          — LocalNodeService (TCP accept loop, full impl)
    ├── remote_node.rs         — RemoteNodeService (per-peer skeleton)
    └── task_manager.rs        — TaskManagerService (orchestrator skeleton)
```

The 6,200-line actor state machines in `neo-core` are *not yet*
ported. The new services in `neo-network` are the *canonical home*
going forward; the legacy actor code will be deleted in Stage F
once the consumer migration (Stage E) is complete.

## Tasks

- [x] 1.1 Survey the actor code (3 modules, ~6,200 lines).
- [x] 1.2 Inventory consumers (neo-rpc, neo-node, neo-consensus).
- [x] 1.3 Confirm `NetworkService` trait in `neo-runtime` (Stage A).
- [x] 2.1 Add deps to `neo-network/Cargo.toml`.
- [x] 2.2 Create `neo-network/src/error.rs` with `NetworkError`.
- [x] 2.3 Create `neo-network/src/event.rs` (re-exports runtime event).
- [x] 2.4 Create `neo-network/src/command.rs` with `NetworkCommand` enum.
- [x] 2.5 Create `neo-network/src/handle.rs` with `NetworkHandle`.
- [x] 2.6 Create `neo-network/src/peer_id.rs` with `PeerId`.
- [x] 3.1 Create `LocalNodeService` (TCP accept loop + `NetworkService` impl).
- [x] 3.2 Create `RemoteNodeService` skeleton + `run()` loop.
- [x] 3.3 Create `TaskManagerService` skeleton + `run()` loop.
- [x] 4.1 Update `neo-network/src/lib.rs` with module exports.
- [x] 5.1 Verify `cargo check --workspace` is 0 errors. **PASS** (0 errors).
- [x] 5.2 Verify `cargo test -p neo-network` passes. **PASS** (5 integration + 1 doctest).
- [x] 5.3 Verify `cargo test -p neo-core --features runtime --lib` has no regression. **PASS** (575 passed, 0 failed, 2 ignored).
- [ ] 6.1 Update `ARCHITECTURE.md` to describe the new services.

## Verification results

- `cargo check --workspace` -> **0 errors** (only warnings, mostly
  pre-existing in `neo-core` and doc-comment lints in the new
  `neo-network` modules).
- `cargo test -p neo-network` -> **5 integration tests pass + 1 doctest**.
- `cargo test -p neo-core --features runtime --lib` ->
  **575 passed, 0 failed, 2 ignored** (matches the Stage B baseline).

## New service API

### `LocalNodeService` (`neo-network/src/local_node.rs`)

```rust
let settings = Arc::new(ProtocolSettings::default());
let (service, handle) = LocalNodeService::new(settings);
let task = tokio::spawn(service.run());

handle.start("127.0.0.1:10333".parse()?).await?;
handle.connect_peer("seed.neo.org:10333".parse()?).await?;
handle.broadcast_block(&block).await?;
handle.broadcast_transaction(&tx).await?;
handle.disconnect_peer(peer_id).await?;
handle.shutdown().await?;
```

Implements the runtime-level [`NetworkService`] trait so it can be
plugged straight into a `NodeBuilder::with_network(...)`:

```rust
let network: Arc<dyn NetworkService> = Arc::new(LocalNodeService::new(settings).0);
```

### `RemoteNodeService` (`neo-network/src/remote_node.rs`)

Each accepted TCP connection spawns one task. The per-peer service
is constructed by `LocalNodeService::handle_start` / `handle_connect_peer`
today; eventually it will be re-used by the ported handshake code.

### `TaskManagerService` (`neo-network/src/task_manager.rs`)

```rust
let (service, handle) = TaskManagerService::new();
let task = tokio::spawn(service.run());
let id = handle.add_task(SyncTask::FetchBlock { hash, kind: SyncTaskKind::Block }).await?;
let active = handle.active_tasks().await?;
handle.complete_task(id, peer_id).await?;
```

## Deferred to later stages

- **Full state-machine port.** The 6,200 lines of `local_node/actor.rs`,
  `remote_node/`, and `task_manager/` logic (handshake state machine,
  bloom filter sync, inventory queue, outbound command queue, peer
  scheduling, completion flow) are NOT ported in this change. The new
  services are skeletal implementations that own the right types and
  the right command loops, but the message handlers are minimal
  stubs. Porting each one is its own multi-hundred-line change.
- **Consumer migration.** `neo-rpc`, `neo-node`, and `neo-consensus`
  continue to use the actor `LocalNodeHandle` / `RemoteNodeHandle` /
  `TaskManagerHandle`. Migrating them to the new service handles is
  Stage E.
- **Remove the actor code.** The actor modules in `neo-core` stay
  until the consumer migration is complete. Deletion is Stage F.
- **Spec / design doc.** A full `design.md` and `specs/` artefacts
  for this change will be added in a follow-up commit; this PR is
  the foundation.
