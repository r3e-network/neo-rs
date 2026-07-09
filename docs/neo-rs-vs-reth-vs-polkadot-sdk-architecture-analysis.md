# neo-rs vs Reth vs Polkadot SDK — Architecture Comparison

> Comparative analysis of neo-rs (Neo N3 Rust node), Reth (Rust Ethereum),
> and Polkadot SDK (Substrate) across storage, sync, pipeline, task management,
> and networking dimensions. Each section identifies what neo-rs can learn.

---

## 1. Storage Architecture

### neo-rs (current)

Single `Store` trait with `DataCache` + `StoreCache` overlay. MDBX is the
production default, RocksDB remains supported, and in-memory providers cover
tests/ephemeral nodes. The ledger read boundary has a hot native-record
provider, a hot/cold router, and an explicit `EmptyLedgerProvider` for nodes
without an installed cold archive, so composition roots can keep one provider
shape before static files land. Current-tip reads are exposed as the separate
`ChainTipProvider` capability, and raw transaction-state records (including
conflict stubs) are exposed as `TransactionStateProvider`, keeping RPC and
peer-serving code on the same provider seam instead of reaching into the native
Ledger contract directly.
Raw key/value bytes remain C# compatible through
`StorageKey` / `StorageItem`, and `StorageKey` / `KeyBuilder` over those raw
bytes is the live encoding on every storage access path.
`neo_storage::persistence::Table`, `TableCodec`, and `TableReader` add a typed
table boundary over those same bytes, but that boundary is on no live storage
access path today — it is a defined seam, not a live encoding.

```rust
pub trait Store: ReadOnlyStore + RawReadOnlyStore + WriteStore + Send + Sync + Any {
    fn snapshot(&self) -> Arc<dyn StoreSnapshot>;
    fn try_commit_raw_overlay(&self, overlay: &[(Vec<u8>, Option<Vec<u8>>)]) -> ...;
}
```

| Aspect | neo-rs | Reth | Polkadot SDK |
|--------|--------|------|-------------|
| Abstraction | Single `Store` trait | `Database` trait with GATs | `kvdb` trait, 3-level stack |
| Table encoding | Live: `StorageKey` / `KeyBuilder` over raw C#-compatible bytes; `Table`/`TableCodec` boundary exists but on no live access path | Per-table `Encode`/`Decode` + `Compact` derive | Per-column encoding (parity-scale-codec) |
| Tiering | Hot DB today; hot/cold provider factory exists, while static-file archive writing/format is still unwired | Hot (MDBX) / Cold (RocksDB) / Static (NippyJar) | Single DB (parity-db or RocksDB) |
| Overlay | `DataCache` with `Arc<RwLock<HashMap>>` | MDBX transaction | `OverlayedChanges` |
| Pruning | Manual via `MaxTraceableBlocks` | Per-segment config (4 profiles) | `PruningMode` enum |
| Static files | Static-file archive writer/format remains future work; provider routing can already compose either a cold implementation or the explicit clean-miss `EmptyLedgerProvider` | NippyJar columnar (mandatory, mmap) | None |

### Reth innovations

1. **Compact derive macro** — auto-generates bitfield-packed encoding for any
   struct. Removes zero bytes from empty hashes/optionals. Saves 15-30% on
   storage.

2. **Hot/Cold/Static three-tier** — MDBX for hot state, RocksDB for indices,
   NippyJar columnar files for old data. Different mount points (NVMe/HDD).

3. **NippyJar** — append-only, mmap-friendly, columnar compression. Block-
   aligned segments (8K blocks per file). Zero-copy reads.

4. **ExEx-aware pruning** — only prunes data after all Execution Extensions
   have acknowledged the height.

### Polkadot SDK innovations

1. **`OverlayedChanges` pattern** — transactional write overlay for runtime
   execution. Reads/writes go to overlay, then committed to trie backend in a
   `BlockImportOperation`. ACID-like semantics.

2. **`PruningMode`** — `ArchiveAll` (keep all state), `ArchivePruned` (specified
   depth), `BlocksPruning` (keep N recent full states). Clean enum.

### Recommendations for neo-rs

| Priority | Change | Benefit |
|----------|--------|---------|
| P0 | Hot/Cold/Static tiering | 30%+ disk savings, tiered hardware |
| P1 | Compact derive macro for Neo types | 15-25% storage savings, fewer bytes |
| Partially wired | Hot/cold ledger provider factory | Provider routing plus explicit empty-cold provider are implemented; append-only static-file archive writing and recovery are still future work |
| P3 | `OverlayedChanges`-style transactional overlay | Cleaner per-tx isolation |

---

## 2. Sync / Pipeline

### neo-rs (current)

Blocks processed sequentially through a single `BlockchainService` command loop.
Out-of-order blocks parked in `Arc<Mutex<BTreeMap<u32, UnverifiedBlocksList>>>`.
Drain batch size = 500, cache max = 50K. Eviction drops top 25% (O(n log n)).
`neo_runtime::sync_pipeline` provides stage identifiers, Reth-style
`CommitPolicy` thresholds (`max_blocks`, `max_changes`, `max_cumulative_gas`,
`max_duration`), a provider-neutral `SyncStageCheckpointStore` seam,
`InMemorySyncStageCheckpointStore` for tests/scaffolding,
`StoreSyncStageCheckpointStore` for store-backed durable checkpoints, and
`SharedStoreSyncStageCheckpointStore` for node-composed `Arc<dyn Store>`
backends. `neo_system::SyncImportPipeline` binds `BlockchainHandle`,
`BlockImportQueue`, durable checkpoint storage, and the import-stage
`CommitPolicy` at node construction time, and `NodeBuilder` registers the same
handle in `ServiceRegistry`. `SyncPipelineDriver`, which imports
contiguous `SyncBlockBatch` values through
the shared `ImportQueue` and checkpoints the import stage when policy fires,
can be created from that handle.
The queue/checkpoint handle is now part of production node composition and
service lookup. Production node startup runs a coordinator-backed P2P
`BlockDownloader` over the live `PeerRegistry` and feeds it through
`SyncDownloadImportDriver` into the composed import pipeline. The concrete
multi-stage headers/bodies/execute/index/prune loop remains the next large
integration step.

Live path: P2P Block -> `InboundInventory::Block` -> `neo-node` buffering ->
`BlockchainHandle::submit_inventory_blocks` -> neo-blockchain
`handle_block_inventory_batch` / `persist_block_sequence`. Consensus-produced
blocks use `submit_inventory_block`; extensible payloads use
`submit_inventory_extensible`; local replay startup uses `initialize`. These
typed handle methods keep `BlockchainCommand` construction inside
`neo-blockchain` while the live path intentionally bypasses the generic
queue/driver layer for now. That bypass remains until the queue has an
inventory-aware adapter: the live inventory path owns relay policy,
future-block parking, unverified draining, and mempool maintenance, while
`SyncPipelineDriver` remains a reusable import-stage primitive; production P2P
range sync now feeds real peer batches into that bridge through the
coordinator-backed downloader/import task.

```rust
// Sequential: verify → persist → commit per block
while let Some(cmd) = cmd_rx.recv().await {
    self.dispatch(cmd).await;  // one block at a time
}
```

### Reth innovations

1. **Staged pipeline** — separate stages for headers, bodies, sender recovery,
   execution, hashing, merkle, indexing, pruning. Each stage persists
   checkpoints → crash-resumable.

   ```
   Headers → Bodies → SenderRecovery → Execution → AccountHashing
   → StorageHashing → Merkle → TransactionLookup → HistoryIndex → Prune
   ```

2. **Commit policy per stage** — `max_blocks`, `max_changes`, `max_cumulative_gas`,
   `max_duration`. First threshold hit = commit. Tunable memory/i-o tradeoff.

3. **Pipeline checkpointing** — resume from last committed stage, not genesis.

4. **ETL for bulk inserts** — data sorted in memory/disk before bulk-inserting
   into MDBX. Configurable memory budget.

### Polkadot SDK innovations

1. **Import queue with concurrent verification** — `BasicQueue` spawns N
   concurrent verify tasks (signature verification on rayon/blocking pool),
   feeds results sequentially to import chain.

2. **Warp sync** — downloads full state as key-value pairs, inserted directly
   into trie backend (`StateAction::Skip`). No historical block execution.
   Minutes vs hours/days.

### Recommendations for neo-rs

| Priority | Change | Benefit |
|----------|--------|---------|
| P0 | Staged sync pipeline integration | 3-5x sync speed, crash resume |
| Composed / P2P Wired | Import queue boundary with bounded concurrent `check` | Reusable preverification surface; `BlockchainHandle::check` now shares live stateless import-integrity checks, `neo_system::SyncImportPipeline` constructs/registers the queue, and production P2P sync drains coordinator batches into it |
| Composed / P2P Wired | Commit policy/checkpoint primitives plus import-stage driver | Tunable memory/i-o; durable checkpoint storage is available through `StoreSyncStageCheckpointStore` and `SharedStoreSyncStageCheckpointStore`, node composition creates the import-stage checkpoint handle, and the coordinator-backed P2P download bridge drives `SyncPipelineDriver` |
| P2 | Warp sync / state sync | Minutes to sync instead of hours |

---

## 3. Block Import Chain

### neo-rs (current)

`neo_runtime::BlockImport` is the canonical import trait. `BlockchainHandle`
implements it and routes to the `neo-blockchain` service loop. The reusable
`neo_runtime::BlockImportQueue` runs bounded concurrent `check` calls, then
submits the verified batch to `BlockImport::import_many` in original order.
`BlockchainHandle::check` now performs the live path's stateless import-integrity
checks (hash serialization, block version, transaction merkle root, and duplicate
transaction hashes), so queued preverification rejects malformed blocks before
ordered import without enforcing the dBFT-only production transaction limit.
RPC `submitblock` uses the same preflight before submitting decoded blocks
through `BlockImport::import(..., BlockOrigin::Rpc)`.
Verification-enabled imports then run
`neo-blockchain::VerifiedImportPipeline`, which composes the concrete
`NeoValidateStage` followed by `NeoConsensusWitnessStage` over the same
snapshot used by native persistence. The second stage resolves the previous
trimmed block, reads its `NextConsensus`, and verifies the header witness
through the explicit native-contract provider; trusted `verify: false`
fast-sync package imports keep relying on the block decoder's import-integrity
checks to avoid duplicating merkle/hash work on the hot replay path.
`neo_runtime::SyncPipelineDriver` consumes contiguous sync batches, rejects
height gaps, calls the import queue, and writes import-stage checkpoints
according to `CommitPolicy`. `neo_system::SyncImportPipeline` now composes the
bounded import queue and durable checkpoint provider from the node's
`BlockchainHandle` and shared storage handle, then registers the same handle in
`ServiceRegistry`; production P2P range sync now creates a coordinator-backed
downloader over live peer handles and drives that runtime driver. The live
inventory path still calls
`BlockImport` directly via
`BlockchainHandle::import_many`, driven by neo-blockchain's
`handle_block_inventory`.
Execution, native persistence, state-root updates, and durable commits still
happen only inside `neo-blockchain`.

### Polkadot SDK innovations

1. **Layered `BlockImport` chain** — chain-of-responsibility:

   ```
   Verifier (consensus-specific: BABE/Aura/PoW)
     → BlockImport (consensus-specific)
       → ClientBlockImport (state validation)
         → DB commit
   ```

2. **`ImportQueue` trait** — `push_blocks()` accepts `IncomingBlock`s,
   verifier checks pre-state, then feeds to import chain.

3. **`ForkChoiceStrategy`** — `UseLongestChain` or `Custom`. Enables pluggable
   fork-choice rules.

### Recommendations for neo-rs

```rust
// Proposed BlockImport chain for neo-rs
let import_chain = VerifiedImportPipeline::new(...) // validate -> dBFT witness
    .and_then(Box::new(StateVerifier))       // balance, nonce, conflicts
    .chain(Box::new(ClientBlockImport));     // persist to DB
```

Current status: the shared trait, bounded queue, and import-stage sync driver
exist as primitives; node composition constructs a `SyncImportPipeline` handle
with the queue and store-backed checkpoints and registers it for service
lookup, while `SyncDownloadImportDriver` drains production coordinator-backed
peer batches into the import-stage driver.
`neo-network::BlockDownloadBatch` converts into `neo_runtime::SyncBlockBatch`,
preserving the single ordered import path. `PeerSession` uses the per-peer
`BlockRequestScheduler`, and the transport-agnostic
`BlockDownloadCoordinator` now composes `CrossPeerBlockRangeScheduler` with the
ordered response buffer behind the `BlockDownloader` stream boundary.
`Arc<PeerRegistry>` implements `BlockRangeFetcher` by resolving the assigned
peer handle, issuing `GetBlockByIndex`, and collecting matching block frames
into a `BlockDownloadBatch`; node composition shares/registers that registry
and the registry exposes advertised-height download snapshots. Production
startup now runs a supervised coordinator-backed downloader/import task while
the broader staged pipeline remains future work.

---

## 4. Task Manager / Service Lifecycle

### neo-rs (current)

`neo-node` now has explicit daemon task supervision (genuinely done). Essential
tasks request node shutdown on error or unexpected exit; normal tasks report/log
failures without terminating the daemon. Metrics use bounded `task_kind` and
`outcome` labels.

```rust
// neo-node/src/node/tasks/supervision.rs
pub(in crate::node) enum TaskKind {
    Essential, // exit/panic cancels the node shutdown token
    Normal,    // exit/panic is reported but does not stop the daemon
}

pub(in crate::node) fn spawn_daemon_task<F>(
    handles: &mut Vec<tokio::task::JoinHandle<()>>,
    observability: Option<&ObservabilityRuntime>,
    shutdown: &CancellationToken,
    kind: TaskKind,
    task_name: &'static str,
    future: F,
) where F: Future<Output = ()> + Send + 'static;

// spawn_daemon_task_result takes the same args with
// F: Future<Output = anyhow::Result<()>> and records an error outcome on Err.
pub(in crate::node) fn spawn_daemon_task_result<F>(/* ... */);
```

### Polkadot SDK innovations

1. **`TaskManager` with supervision** — hierarchical shutdown, essential task
   monitoring.

   ```rust
   TaskManager {
       spawn_handle()           → SpawnTaskHandle (non-essential)
       spawn_essential_handle() → SpawnEssentialTaskHandle (fails → node dies)
       future()                 → completes on termination signal
       add_child()              → parent stops → children stop
   }
   ```

2. **Essential vs non-essential** — consensus, block import, network are
   essential (their failure terminates node). Telemetry, metrics are not.

### Reth innovations

1. **Tokio task model** — each major component runs as a `tokio::spawn`'d task
   communicating via `mpsc` / `broadcast` channels.

2. **`NodeBuilder` pattern** — type-level composition with
   `with_types::<EthereumNode>().with_components(...).launch()`.

### Recommendations for neo-rs

```rust
pub struct NodeTaskManager {
    essential: CancellationToken,
    non_essential: CancellationToken,
    health: Arc<AtomicBool>,
    handles: Vec<JoinHandle<()>>,
}
// Consensus failure → cancel essential → node shuts down
// RPC failure → just log
```

---

## 5. Networking / Peer Management

### neo-rs (current)

`LocalNodeService` owns TCP accept loop, spawns `RemoteNodeService` per-peer.
Incoming blocks routed through `mpsc` to blockchain service. Block requests are
planned by a reusable per-peer scheduler, while received bodies still arrive via
wire `block` messages and the inventory sink. The
`neo_network::BlockDownloader` stream boundary and `BlockDownloadConfig` policy
records now exist, with a channel-backed adapter for tests/composition roots.
`BlockDownloadBatch` converts into `neo_runtime::SyncBlockBatch`, which the
runtime sync driver can feed into the import queue. `neo_system` provides
`SyncDownloadImportDriver`, which drains any `BlockDownloader` stream into the
node-composed `SyncImportPipeline` and surfaces downloader/import errors through
the shared runtime error vocabulary. The per-peer
`BlockRequestScheduler` owns the `GetBlockByIndex` request-window policy
used by `PeerSession` (`500` blocks per request, `1000` blocks in flight,
stall rewind).
`CrossPeerBlockRangeScheduler` (cross-peer selection, peer bias, bounded
in-flight range assignment, retry accounting) and `OrderedBlockBatchBuffer`
(holds out-of-order peer responses until the next contiguous height is
available) are composed by `BlockDownloadCoordinator`, a transport-agnostic
`BlockDownloader` stream over any `BlockRangeFetcher`. Live peer transport now
has a registry-backed fetcher that uses `RemoteNodeHandle::fetch_blocks_by_index`;
`PeerRegistry::download_peers` provides advertised-height peer snapshots, and
the node composition root registers the shared registry. `ChannelBlockDownloader`
remains available for tests and composition roots. Node startup now disables
legacy per-peer automatic block requests and runs the coordinator-backed
downloader/import driver as the production owner of P2P range sync.

### Reth innovations

1. **`HeaderDownloader` as `Stream`** — yields headers as they arrive.
   Configurable concurrency (5-100 in-flight requests). Retry per request.

2. **Bodies downloader** — buffers up to 2 GB before writing. Parallelized
   across peers via `GetBlockBodies`.

3. **Stream-based architecture** — pipeline doesn't wait for full download.
   Processes in configurable commit batches.

### Polkadot SDK innovations

1. **Notification protocols** — unidirectional substreams for GRANDPA,
   transactions, block announces. `NotificationService` trait for custom
   protocols.

2. **Request-response protocols** — light client requests, state proofs.
   Pluggable protocol handlers.

### Recommendations for neo-rs

```rust
// Implemented downloader boundary
pub trait BlockDownloader:
    Stream<Item = NetworkResult<BlockDownloadBatch>> + Send + Unpin
{
    fn config(&self) -> &BlockDownloadConfig;
    fn poll_next_batch(...) -> Poll<Option<NetworkResult<BlockDownloadBatch>>>;
}
// WIRED: per-peer compatibility policy (BlockRequestScheduler, used by PeerSession).
// WIRED: cross-peer range policy + retry accounting (CrossPeerBlockRangeScheduler).
// WIRED: contiguous response release (OrderedBlockBatchBuffer).
// WIRED: transport-agnostic coordinator stream (BlockDownloadCoordinator).
// WIRED: live peer fetching through Arc<PeerRegistry>: BlockRangeFetcher.
// WIRED: SyncDownloadImportDriver drains BlockDownloader into SyncImportPipeline.
// Node startup owns range sync through the coordinator-backed downloader/import task.
```

---

## 6. Execution / VM

### neo-rs (current)

Native Rust NeoVM — no WASM. `ApplicationEngine` with per-tx child caches.
`NativeContractProvider` remains the lower-level execution seam, but
`neo-system::NodeBuilder` now makes the provider an explicit composition-root
dependency. The daemon constructs the standard provider once before genesis
initialization and passes the same `Arc` into every provider-aware subsystem and
into `NodeBuilder`; headless/test construction still falls back to the builder's
local standard provider default. `ApplicationEngine` now captures the explicit
or scoped provider at construction and uses that stable handle for direct native
calls, policy reads, dynamic-call policy gates, contract-management lookups made
from contract loading, committee-witness checks, storage-context resolution,
OracleResponse witness inheritance, witness group checks, current-index reads,
and whitelisted-fee checks. Engine methods do not read the global provider after
construction, so later provider replacement cannot affect an already-created
engine. Production composition no longer mutates the process-global provider
slot. Runtime witness helpers now have explicit-provider entry points. Native
block persistence passes
`NativePersistResources` providers directly into OnPersist/Application/PostPersist
engines, and service-level genesis initialization plus batch resource setup
build those resources from `SystemContext::native_contract_provider`; live block
import uses the explicit-resource staging/commit path instead of the global
provider. `NativeContractLookup` is now reduced to a compatibility bridge for
installing, replacing, scoping, and reading the ambient provider; the
contract-specific global helper wrappers were removed from
`neo-execution/src/native/native_contract_provider.rs`. Mempool admission
adapts the `MemoryPool`-captured provider for Policy, GAS, Notary, NEO,
Oracle, and RoleManagement reads instead of constructing a private native
provider factory, so transaction verification observes the same native-contract
set as block import, consensus, RPC, and state-root verification. RPC session
construction, smart-contract wallet invocation, wallet-compat network-fee
calculation, RPC wallet signing/finalization, and RPC node `getversion` policy
projection now follow the same rule for Policy reads: they adapt the composed
provider passed into their execution path for max-valid-until-block,
milliseconds-per-block, execution-fee-factor, fee-per-byte, and dynamic
`getversion` protocol values instead of constructing standalone
`PolicyContract` handles or duplicating Policy storage keys through local
native factories. Those RPC Policy adapters share a crate-local adapter for
registry lookup and downcasting, leaving each endpoint module with only its
narrow capability trait. Oracle service processing also adapts the `OracleService`-owned
`NativeContractProvider` for Oracle, ContractManagement, RoleManagement, and
Policy reads instead of constructing private native handles or a service-local
native factory.

### Polkadot SDK innovations

1. **WASM runtime as meta-protocol** — runtime blob in state `:code` is source
   of truth. Client is a "game console" for WASM "games".

2. **WASM instance cache** — compiled modules cached by code hash.
   `Pooling` instantiation strategy pre-allocates instances.

3. **`NativeElseWasmExecutor`** — native fast path with WASM fallback on
   version mismatch.

### Recommendations for neo-rs

1. Keep native execution (NeoVM in Rust is already fast).
2. **Partial:** `NativeContractProvider` is now an explicit `NodeBuilder` field,
   so the composition root chooses the provider. The daemon now reuses one
   standard provider for early genesis/native persistence and the composed
   `Node`, and `ApplicationEngine` captures the provider during construction for
   direct native calls, policy reads, dynamic-call policy gates,
   contract-management lookups made from contract loading, committee-witness
   checks, storage-context resolution, OracleResponse witness inheritance,
   witness group checks, current-index reads, and fee whitelist checks. Engine
   methods no longer read the global provider after construction.
   Batch block import, genesis initialization, header inventory verification,
   extensible-payload verification, and signed-StateRoot verification now use
   explicit providers when their caller owns one; native persistence exposes an
   explicit-resource committing helper, and production composition no longer
   installs the standard provider globally. `NativeContractLookup` now only
   exposes provider install/replace/scope/access helpers, so callers must use
   the provider trait for concrete native lookups instead of contract-specific
   global wrappers. Mempool admission now follows that same rule: its native
   read capability is an adapter over the composed provider, with only the
   ledger-storage read capability left behind its separate provider factory.
   Oracle service request processing and response construction also adapt the
   `OracleService`-owned provider for Oracle/ContractManagement/RoleManagement/
   Policy reads, so off-chain oracle work observes the same native-contract set
   as the rest of the node.
3. Consider WASM runtime for future sidechain/feature-gate support.

---

## 7. Codec / Serialization

### neo-rs (current)

`StorageKey` + `StorageItem` with `KeyBuilder` for encoding. Binary/JSON
serializers in `neo-serialization`.

### Reth innovations

1. **`Table` trait with `Encode`/`Decode`/`Compress`/`Decompress`** — generic
   over every table's Key/Value. Swappable encoding per table.

2. **`Compact` derive macro** — auto-generated bitfield packing. Uses
   `modular_bitfield` to squeeze out zero bytes.

3. **Codec variants** — `Compact` (default), `Scale` (parity-scale-codec),
   `Postcard` (serde), `Passthrough` (raw bytes).

### Recommendations for neo-rs

```rust
// Proposed Table trait
pub trait Table: Send + Sync + 'static {
    type Key: Encode + Decode;
    type Value: Encode + Decode;
    const NAME: &'static str;
}

// Compact derive for Neo types
#[derive(Compact)]
pub struct TransactionState {
    pub block_index: u32,
    pub vm_state: VMState,          // 1 byte
    pub transaction: Option<Transaction>,  // optional → 0 bytes when None
}
```

---

## Summary: Impact Matrix

| Change | Speed | Storage | Reliability | Complexity | Effort |
|--------|-------|---------|-------------|------------|--------|
| Staged sync pipeline integration | ★★★★★ | ★★ | ★★★★ | ★★★★ | Large |
| Hot/Cold/Static tiering | ★★★ | ★★★★★ | ★★★ | ★★★ | Medium |
| Import queue + concurrent verify | ★★★★ | - | ★★★ | ★★★ | Runtime queue and production download-to-import bridge wired |
| Stage commit policy + checkpoints | ★★★ | - | ★★★★ | ★★ | Import-stage driver done |
| Compact derive macro | ★★ | ★★★★ | - | ★★ | Small |
| Task supervision | - | - | ★★★★★ | ★★ | Done |
| BlockDownloader as Stream | ★★★ | - | ★★ | ★★★ | Boundary, coordinator, registry-backed peer fetcher, peer snapshots, and startup driver wired |
| Essential task monitoring | - | - | ★★★★★ | ★ | Done |
| Metrics infrastructure | - | - | ★★★★ | ★★ | Medium |

---

## Implementation Order (Recommended)

1. **Essential task supervision + metrics** — implemented in `neo-node`.
2. **Typed table boundary** — implemented in `neo-storage` but on no live storage access path (the live encoding remains `StorageKey` / `KeyBuilder` over raw C#-compatible bytes); compact derive is still future work.
3. **Block import queue with concurrent verification** — reusable runtime boundary implemented and composed by `neo_system::SyncImportPipeline`.
4. **Commit policy/checkpoint primitives and import driver** — implemented in `neo-runtime::sync_pipeline`; durable store-backed checkpoints are available through `StoreSyncStageCheckpointStore` and `SharedStoreSyncStageCheckpointStore`, node composition creates the import-stage queue/checkpoint handle, and `SyncDownloadImportDriver` now receives production P2P coordinator batches.
5. **BlockDownloader as Stream** — implemented in `neo-network`; batches convert to `SyncBlockBatch`; `neo-system` has the download-to-import bridge; per-peer `BlockRequestScheduler` remains as the legacy compatibility path; `BlockDownloadCoordinator` composes `CrossPeerBlockRangeScheduler` (cross-peer assignment/retry policy) and `OrderedBlockBatchBuffer` (contiguous response release) behind a transport-agnostic `BlockRangeFetcher`; `Arc<PeerRegistry>` implements live peer fetching through `RemoteNodeHandle::fetch_blocks_by_index`; `PeerRegistry::download_peers` exposes advertised-height peer snapshots; node composition registers the shared registry, disables legacy per-peer block requests, and starts the coordinator-backed downloader/import task.
6. **Hot/Cold/Static tiering integration** (medium, big storage win)
7. **Staged sync pipeline integration** (large, biggest overall impact)

This document is a living reference — update as architecture evolves.
