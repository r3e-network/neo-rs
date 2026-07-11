# neo-rs vs Reth vs Polkadot SDK — Architecture Comparison

> Comparative analysis of neo-rs (Neo N3 Rust node), Reth (Rust Ethereum),
> and Polkadot SDK (Substrate) across storage, sync, pipeline, task management,
> and networking dimensions. Each section identifies what neo-rs can learn.

---

## 1. Storage Architecture

### neo-rs (current)

Single `Store` trait with `DataCache` + `StoreCache` overlay. MDBX is the
production default, RocksDB remains supported, and in-memory providers cover
tests/ephemeral nodes. The canonical `SystemContext::commit_to_store` boundary
propagates backend failures from `StoreCache::try_commit_durable`; failed
overlays are discarded, staged bulk tips are rewound, and post-commit callbacks
are emitted only after the durable fence succeeds. Accepted bulk prefixes
therefore cannot be reported from an unflushed shared snapshot. MDBX implements
that fence with its atomic write transaction; RocksDB uses a WAL-synchronous
overlay batch and persists any earlier WAL-disabled fast-sync prefix first.
StateService and a persistent indexer remain separate durability domains, so
neo-rs uses write-ahead fail-stop recovery rather than claiming cross-store
atomicity: it writes and fsyncs a poison marker before either observer can
publish, fences both observer stores before Ledger, and removes the marker only
after canonical success. A crash or failure leaves the marker for startup to reject
(including the uninitialized-chain/genesis-root mismatch). ApplicationLogs and
TokensTracker persist post-canonical and avoid this marker cost. Canonical
durability failure always stops the active writer immediately. The ledger read
boundary now has hot native-record, static-file, optional, and hot/cold routed
providers behind the same capability traits and GAT factory shape.
`neo-static-files` supplies versioned append-only height frames, zstd
compression, xxh3 checksums, continuity enforcement, an existing-crate LRU
frame cache, torn-tail repair, and a kernel-owned writer lease retained by all
provider clones. Frames begin at genesis, and only one process can perform
recovery or append for an archive at a time. A path-adjacent MDBX sidecar stores
checksummed frame boundaries and every height-versioned row location; the
archive header carries a non-zero identity that prevents reuse of a stale
sidecar after file replacement. Publication is strictly archive append and
`sync_data` first, followed by one durable sidecar transaction. Clean open
validates the published tail and scans only an unpublished suffix. An ahead,
missing, or incompatible sidecar is rebuilt from authoritative archive frames,
while truncation deletes discarded versions so an overwritten key exposes its
latest retained value without a genesis rescan. Normal lookup verifies the
complete compressed frame before returning bytes, and explicit strict scrub
checks every frame and index entry. `StaticLedgerArchive` captures exact
C#-compatible Ledger rows after execution; node commit hooks keep those rows in
memory until the precommit durability fence writes and syncs the whole accepted
batch without publishing its index, then commit canonical MDBX/RocksDB state
and expose the staged frames through one sidecar transaction. Startup validates
every still-hot overlapping block hash against the canonical suffix, truncates
a recovered cold-ahead suffix, and repairs archive lag above the prune
watermark before composition
installs `StaticLedgerProvider` as the cold side. That same statically
dispatched optional provider now reaches blockchain-service fallback reads,
node and RPC transaction admission, dBFT tip/transaction/conflict checks, local
P2P serving, wallet transaction-state reads, and historical RPC block and
transaction queries. Archive publication is cold-first and crash-reconcilable,
and hot-row pruning is now version-aware and atomic. The archive enumerates a
frame's keys without payload decompression and batch-resolves the latest height
for each key in one sidecar MDBX read transaction. Only keys whose latest
version is at or below the bounded frontier are eligible, and hot/cold bytes
must match before deletion. MDBX uses one transaction across its data and
node-metadata tables; RocksDB uses one synchronous batch across its default and
metadata column families. The initial protocol `MaxTraceableBlocks` is the hot
retention floor, while `CurrentBlock` remains hot. The delete set and
`hot-pruned-through` watermark become durable atomically. Startup rejects a
watermark above canonical/archive truth or archive lag below a pruned prefix,
then validates overlap from `watermark + 1`. Persistent offsets, bounded
archive open, archive-aware offline tooling, atomic hot deletion, and
prune/recovery parity are complete; segment rotation remains future work.
Operational persisted-tip reads (startup, config validation, chain.acc
resume, and daemon context) share that routed factory shape. Observability
ledger-height reads (health/readiness/metrics) use the same boundary for
local-ledger mode while remote-ledger mode reports the upstream RPC height.
Composition-root transaction admission also uses the routed factory shape for
persisted-transaction and conflict checks before it adapts the mempool-captured
native-contract provider for Policy reads. Consensus orchestration uses the
same shape for tip context, on-chain transaction checks, and traceable-conflict
checks before adapting the node-composed native-contract provider for NEO and
Policy reads.
Blockchain ingress validation also uses the routed factory shape for header
anchor reads and extensible-payload height checks before applying witness and
native-provider validation.
Blockchain transaction admission uses the same shape for persisted transaction
and conflict checks before calling into mempool policy.
Offline `neo-db-probe` replay accepts `--static-files-dir` and composes the
same `OptionalStaticLedgerProvider` through `HotColdLedgerProviderFactory` for
transaction-state and block reconstruction. Before exposing cold reads it runs
the same hot-prefix reconciliation used by node startup, repairing lag/ahead
tails and rejecting fork mismatch. It also routes explicit raw Ledger
transaction-row probes to the archive after a clean hot miss, and its
archive-only `--scrub-static-files` path verifies frame/index parity without
opening the canonical database. When it opens a hot database, it also reads the
isolated prune watermark before reconciliation so archive-backed history works
after hot pruning.
Durable store fallback reads after in-memory block-cache eviction use the same
routing for block-hash and full-block reconstruction.
Blockchain and wallet transaction-state adapters use the node-composed routed
factory before projecting JSON-RPC responses; the shared RPC ledger-query
helper uses it for historical blocks, headers, and verbose transaction context.
RPC session dummy-block and other current-tip reads deliberately use the hot
current-block record because that record is never cold-tiered.
Current-tip reads are exposed as the separate
`ChainTipProvider` capability, and raw transaction-state records (including
conflict stubs) are exposed as `TransactionStateProvider`, keeping RPC and
peer-serving code on the same provider seam instead of reaching into the native
Ledger contract directly.
Raw key/value bytes remain C# compatible through `StorageKey` / `StorageItem`,
and `StorageKey` / `KeyBuilder` over those raw bytes is the live encoding on
every storage access path. `Store`, `RawReadOnlyStore`,
`ReadOnlyStoreGeneric`, and `StoreFactory` are the implemented backend seams;
a Reth-style typed table/codec layer remains future work and must adapt these
existing bytes rather than redefine them.

```rust
pub trait Store: ReadOnlyStore + RawReadOnlyStore + WriteStore + Send + Sync {
    type Snapshot: StoreSnapshot;

    fn snapshot(&self) -> Arc<Self::Snapshot>;
    fn try_commit_raw_overlay(&self, overlay: &[(Vec<u8>, Option<Vec<u8>>)]) -> ...;
}
```

| Aspect | neo-rs | Reth | Polkadot SDK |
|--------|--------|------|-------------|
| Abstraction | `Store` with an associated concrete snapshot; `RuntimeStore` enum for config selection | `Database` trait with GATs | `kvdb` trait, 3-level stack |
| Table encoding | Live: `StorageKey` / `KeyBuilder` over raw C#-compatible bytes; no separate typed-table API yet | Per-table `Encode`/`Decode` + `Compact` derive | Per-column encoding (parity-scale-codec) |
| Tiering | Hot MDBX/RocksDB plus opt-in compressed static Ledger archive; provider routing, atomic pruning, and watermark-aware recovery are wired | Hot (MDBX) / Cold (RocksDB) / Static (NippyJar) | Single DB (parity-db or RocksDB) |
| Overlay | `DataCache` with `Arc<RwLock<HashMap>>` | MDBX transaction | `OverlayedChanges` |
| Pruning | Static Ledger rows automatically prune beyond the initial `MaxTraceableBlocks` window; state/MPT pruning remains separately configured | Per-segment config (4 profiles) | `PruningMode` enum |
| Static files | Versioned per-height zstd frames, opaque exact Ledger rows, checksums, LRU reads, continuity and torn-tail recovery; cold-first bytes with post-hot index visibility | NippyJar columnar (mandatory, mmap) | None |

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
| Implemented phase | Atomic hot-row pruning and replay/recovery parity | Disk savings with latest-version filtering, byte parity checks, isolated watermarks, and fail-closed startup |
| P1 | Compact derive macro for Neo types | 15-25% storage savings, fewer bytes |
| Implemented phase | Static Ledger archive and provider routing | Format, exact row capture, cold-first batch publication, startup reconciliation, shared runtime cold reads, and hot pruning are wired |
| P3 | `OverlayedChanges`-style transactional overlay | Cleaner per-tx isolation |

---

## 2. Sync / Pipeline

### neo-rs (current)

Blocks processed sequentially through a single `BlockchainService` command loop.
Out-of-order blocks are parked in a single-lock `UnverifiedBlockCache` backed
by height-ordered buckets with exact O(1) block-count accounting. Drain batch
size = 500 and cache max = 50K. On overflow it evicts exactly 25% of blocks
from the farthest-future heights without allocating a key list; multiple fork
candidates at one height no longer cause the whole bucket to be dropped.
`neo_runtime::sync_pipeline` provides stage identifiers, Reth-style
`CommitPolicy` thresholds (`max_blocks`, `max_changes`, `max_cumulative_gas`,
`max_duration`), a provider-neutral `SyncStageCheckpointStore` seam,
`InMemorySyncStageCheckpointStore` for tests/scaffolding,
`StoreSyncStageCheckpointStore` for store-backed durable checkpoints, and
`SharedStoreSyncStageCheckpointStore<S>` for node-composed `Arc<S>` shared
stores, preserving concrete backend types when composition knows them and using
the concrete `RuntimeStore` enum at runtime-selected backend boundaries.
`neo_system::SyncImportPipeline` binds `BlockchainHandle`,
`BlockImportQueue`, durable checkpoint storage, and the import-stage
`CommitPolicy` at node construction time. `Node` owns that handle as an
explicit typed field. `SyncPipelineDriver`, which imports
contiguous `SyncBlockBatch` values through
the shared `ImportQueue` and checkpoints the import stage when policy fires,
can be created from that handle.
The queue/checkpoint handle is now part of production node composition.
Production node startup runs a coordinator-backed P2P
`BlockDownloader` over the live `PeerRegistry` and feeds it through
`SyncDownloadImportDriver` into the composed import pipeline. `Import` is now
followed by one honest production-consumed `Index` stage in `neo-node`. That
statically dispatched stage snapshots a canonical target, resumes from the
indexer's hash-verified contiguous `IndexerStatus`, processes committed blocks
in bounded atomic batches, and fences each batch. It prunes an ahead index and
durably clears a divergent/incomplete projection before rebuilding. The
indexer's own status remains authoritative because its service store cannot be
atomically checkpointed with canonical Ledger storage. Execution, native
persistence, and state-root work remain inside `Import`; neo-rs does not invent
fake stages for work already durably completed there. Separate headers/bodies
and pruning stages remain future work.

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
| Implemented | Promote `Index` as the next production-consumed stage after `Import` | Durable bounded projection resume without inventing fake execution/state-root stages or a cross-store checkpoint |
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
through the explicit native-contract provider. `ImportMode::Sync` always uses
that verified path. Trusted local package replay uses
`ImportMode::TrustedReplay { verify: false }`, keeps decoder integrity checks,
and alone suppresses replay artifacts and live side effects. Before mutation,
`ImportPlan` resolves a range-aware `SyncBatchCommitPolicy`: eligible peer
batches share one durable commit while retaining ordered hooks, mempool
updates, import events, and one batch-end reverify; otherwise they use per-block
durability. The plan freezes live or catch-up observer behavior for the range.
`neo_runtime::SyncPipelineDriver` consumes contiguous sync batches, rejects
height gaps, calls the import queue, and writes import-stage checkpoints only
after durable progress and according to `CommitPolicy`. Store-backed checkpoint
providers use the backend's isolated maintenance metadata rather than magic
keys in the normal Neo data table. Versioned checkpoint updates and obsolete
normal-table key discard use durable `StoreMaintenanceBatch` transactions,
keeping operational metadata out of typed scans, store dumps, and state-root
input. Old checkpoint hints are deliberately not migrated because production
sync realigns to the authoritative canonical tip before downloading.
`SyncDownloadImportDriver` seeds its cursor from the canonical tip and surfaces
downloader, checkpoint-read/write, gap, and partial-import errors.
`neo_system::SyncImportPipeline` now composes the
bounded import queue and durable checkpoint provider from the node's
`BlockchainHandle` and shared storage handle as one explicit node field;
production P2P range sync now creates a coordinator-backed
downloader over live peer handles and drives that runtime driver. The live
inventory path still calls
`BlockImport` directly via
`BlockchainHandle::import_many`, driven by neo-blockchain's
`handle_block_inventory`.
`BlockImport`, `ImportQueue`, and `NetworkService` return concrete
`impl Future + Send` values. Validation and consensus-witness stages are
synchronous. The hot import path therefore has neither trait-object dispatch
nor `async_trait` future boxing.
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
preserving the single ordered import path. `BlockRequest` carries the protocol
cap, while the transport-agnostic `BlockDownloadCoordinator` composes the sole
`CrossPeerBlockRangeScheduler` with the ordered response buffer behind the
`BlockDownloader` stream boundary. `PeerSession` only executes correlated
range assignments, and the scheduler limits each peer to one in-flight range to
match that response state. Each peer correlation has an absolute deadline that
unrelated traffic cannot refresh. It is accepted only in the `Ready` state and
never enters the generic handshake queue; expiry leaves the connection
available for coordinator-driven retry. The uncomposed network
`TaskManagerService`, per-peer timer scheduler, and fire-and-forget range API
were removed.
`Arc<PeerRegistry>` implements `BlockRangeFetcher` by resolving the assigned
peer handle, issuing `GetBlockByIndex`, and collecting matching block frames
into a `BlockDownloadBatch`; node composition shares/registers that registry
and the registry exposes ready, advertised-height download snapshots.
Production startup now runs a supervised coordinator-backed downloader/import
task and an independently durable committed-chain Index follower. Separate
headers/bodies and pruning stages remain future work.

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

`LocalNodeService` owns the TCP accept loop and spawns one `RemoteNodeService`
per peer. Relayed blocks route through `mpsc` to the blockchain service, while
coordinator-assigned bodies are correlated inside the peer session and returned
as download batches. The
`neo_network::BlockDownloader` stream boundary and `BlockDownloadConfig` policy
records now exist, with a channel-backed adapter for tests/composition roots.
`BlockDownloadBatch` converts into `neo_runtime::SyncBlockBatch`, which the
runtime sync driver can feed into the import queue. `neo_system` provides
`SyncDownloadImportDriver`, which drains any `BlockDownloader` stream into the
node-composed `SyncImportPipeline` and surfaces downloader/import errors through
the shared runtime error vocabulary. `BlockRequest` owns the Neo 500-block wire
cap; there is no autonomous per-peer scheduler.
`CrossPeerBlockRangeScheduler` (cross-peer selection, peer bias, bounded
in-flight range assignment with one range per peer, retry accounting) and
`OrderedBlockBatchBuffer`
(holds out-of-order peer responses until the next contiguous height is
available) are composed by `BlockDownloadCoordinator`, a transport-agnostic
`BlockDownloader` stream over any `BlockRangeFetcher`. Live peer transport now
has a registry-backed fetcher that uses
`RemoteNodeHandle::fetch_blocks_by_index`; `PeerRegistry::download_peers`
provides ready, advertised-height peer snapshots, and the node composition root
registers the shared registry. `ChannelBlockDownloader` remains available for
tests and composition roots. Node startup runs the coordinator-backed
downloader/import driver as the only owner of P2P range sync. The earlier
uncomposed network task manager and per-peer compatibility request loop were
deleted.

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
// WIRED: protocol request cap (BlockRequest::MAX_COUNT).
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
local standard provider default. `ApplicationEngine<P, D, B>` now owns a
mandatory `Arc<P>` and uses that stable handle for direct native
calls, policy reads, dynamic-call policy gates, contract-management lookups made
from contract loading, committee-witness checks, storage-context resolution,
OracleResponse witness inheritance, witness group checks, current-index reads,
and whitelisted-fee checks. Provider-aware constructors and witness helpers no
longer accept `Option<Arc<P>>`; standalone engines use the typed
`NoNativeContractProvider` null implementation. This makes provider ownership a
compile-time invariant and removes missing-provider branches from the execution
hot path. Native block persistence passes
`NativePersistResources` providers directly into OnPersist/Application/PostPersist
engines, and service-level genesis initialization plus batch resource setup
build those resources from `SystemContext::native_contract_provider`; live block
import uses the explicit-resource staging/commit path instead of the global
provider. The obsolete ambient `NativeContractLookup` bridge has been removed;
the provider trait and composition-owned values are the only native lookup
path. Mempool admission adapts the `MemoryPool`-captured provider for Policy,
GAS, Notary, NEO,
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
native factory. The VM's raw, monomorphized host callback pointer is installed
only for context-load or execution operations and cleared before those methods
return, keeping a returned `ApplicationEngine` movable between calls.

### Polkadot SDK innovations

1. **WASM runtime as meta-protocol** — runtime blob in state `:code` is source
   of truth. Client is a "game console" for WASM "games".

2. **WASM instance cache** — compiled modules cached by code hash.
   `Pooling` instantiation strategy pre-allocates instances.

3. **`NativeElseWasmExecutor`** — native fast path with WASM fallback on
   version mismatch.

### Recommendations for neo-rs

1. Keep native execution (NeoVM in Rust is already fast).
2. **Implemented at the execution boundary:** `NativeContractProvider` is an
   explicit `NodeBuilder` field,
   so the composition root chooses the provider. The daemon now reuses one
   standard provider for early genesis/native persistence and the composed
   `Node`, and `ApplicationEngine` requires the provider during construction for
   direct native calls, policy reads, dynamic-call policy gates,
   contract-management lookups made from contract loading, committee-witness
   checks, storage-context resolution, OracleResponse witness inheritance,
   witness group checks, current-index reads, and fee whitelist checks. Engine
   methods no longer perform optional-provider branches. Standalone/test
   engines pass `NoNativeContractProvider` explicitly, and witness helpers
   require `Arc<P>` as well.
   Batch block import, genesis initialization, header inventory verification,
   extensible-payload verification, and signed-StateRoot verification now use
   explicit providers when their caller owns one; native persistence exposes an
   explicit-resource committing helper. The removed ambient lookup bridge
   cannot be used to bypass composition. Mempool admission follows the same
   rule: its native read capability is an adapter over the composed provider,
   with only the
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
| Staged sync pipeline integration | ★★★★★ | ★★ | ★★★★ | ★★★★ | Import and committed-chain Index stages are live; headers/bodies/prune remain |
| Static archive/recovery/provider propagation/hot pruning | ★★★ | ★★★★ | ★★★★ | ★★★ | Implemented; segment rotation remains |
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
2. **Typed table boundary** — not implemented. The live encoding remains
   `StorageKey` / `KeyBuilder` over raw C#-compatible bytes; any future typed
   adapter and compact derive must preserve those bytes.
3. **Block import queue with concurrent verification** — reusable runtime boundary implemented and composed by `neo_system::SyncImportPipeline`.
4. **Commit policy/checkpoint primitives and import driver** — implemented in `neo-runtime::sync_pipeline`; durable store-backed checkpoints are available through `StoreSyncStageCheckpointStore` and `SharedStoreSyncStageCheckpointStore`, persist in isolated maintenance metadata through atomic `StoreMaintenanceBatch` commits, node composition creates the import-stage queue/checkpoint handle, and `SyncDownloadImportDriver` now receives production P2P coordinator batches.
5. **BlockDownloader as Stream** — implemented in `neo-network`; batches convert to `SyncBlockBatch`; `neo-system` has the download-to-import bridge; `BlockDownloadCoordinator` is the single range owner and composes `CrossPeerBlockRangeScheduler` (cross-peer assignment/retry policy) with `OrderedBlockBatchBuffer` (contiguous response release) behind a transport-agnostic `BlockRangeFetcher`; `Arc<PeerRegistry>` implements live peer fetching through the correlated `RemoteNodeHandle::fetch_blocks_by_index` API; each peer fetch is ready-only, bypasses the generic handshake queue, has an absolute deadline independent of connection-idle traffic, and clears its correlation state on expiry; `PeerRegistry::download_peers` exposes ready, advertised-height peer snapshots; node composition registers the shared registry and starts the coordinator-backed downloader/import task. The unused network task manager, per-peer timer scheduler, ownership mode, and fire-and-forget request API were removed.
6. **Hot/Cold/Static tiering integration** — append-only archive, exact Ledger
   adapter, cold-first precommit publication, recovery, persistent archive
   offsets, archive-aware offline tooling, and shared runtime cold reads are
   implemented. Latest-version-aware hot deletion and atomic prune watermarks
   are also implemented; segment rotation remains optional future work.
7. **Staged sync pipeline integration** — `Import` plus the durable
   committed-chain `Index` follower are production-wired. Add only real
   remaining ownership boundaries (headers/bodies and pruning); do not split
   execution or state-root work out of canonical `Import` for symmetry.

This document is a living reference — update as architecture evolves.
