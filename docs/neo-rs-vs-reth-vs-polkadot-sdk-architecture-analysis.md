# neo-rs vs Reth vs Polkadot SDK — Architecture Comparison

> Comparative analysis of neo-rs (Neo N3 Rust node), Reth (Rust Ethereum),
> and Polkadot SDK (Substrate) across storage, sync, pipeline, task management,
> and networking dimensions. Each section identifies what neo-rs can learn.

---

## 1. Storage Architecture

### neo-rs (current)

Single `Store` trait with `DataCache` + `StoreCache` overlay. MDBX is the
production default, RocksDB remains supported, and in-memory providers cover
tests/ephemeral nodes. Raw key/value bytes remain C# compatible through
`StorageKey` / `StorageItem`; `neo_storage::persistence::Table`,
`TableCodec`, and `TableReader` add a typed table boundary over those same
bytes.

```rust
pub trait Store: ReadOnlyStore + RawReadOnlyStore + WriteStore + Send + Sync + Any {
    fn snapshot(&self) -> Arc<dyn StoreSnapshot>;
    fn try_commit_raw_overlay(&self, overlay: &[(Vec<u8>, Option<Vec<u8>>)]) -> ...;
}
```

| Aspect | neo-rs | Reth | Polkadot SDK |
|--------|--------|------|-------------|
| Abstraction | Single `Store` trait | `Database` trait with GATs | `kvdb` trait, 3-level stack |
| Table encoding | `Table`/`TableCodec` over C# bytes; `KeyBuilder` remains for domain keys | Per-table `Encode`/`Decode` + `Compact` derive | Per-column encoding (parity-scale-codec) |
| Tiering | Hot DB today plus provider-backed cold ledger/static-file scaffold | Hot (MDBX) / Cold (RocksDB) / Static (NippyJar) | Single DB (parity-db or RocksDB) |
| Overlay | `DataCache` with `Arc<RwLock<HashMap>>` | MDBX transaction | `OverlayedChanges` |
| Pruning | Manual via `MaxTraceableBlocks` | Per-segment config (4 profiles) | `PruningMode` enum |
| Static files | `StaticLedgerArchive` scaffold via providers (`parking_lot::Mutex`) | NippyJar columnar (mandatory, mmap) | None |

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
| Done | `parking_lot::Mutex` in static files | Eliminates std mutex contention |
| P3 | `OverlayedChanges`-style transactional overlay | Cleaner per-tx isolation |

---

## 2. Sync / Pipeline

### neo-rs (current)

Blocks processed sequentially through a single `BlockchainService` command loop.
Out-of-order blocks parked in `Arc<Mutex<BTreeMap<u32, UnverifiedBlocksList>>>`.
Drain batch size = 500, cache max = 50K. Eviction drops top 25% (O(n log n)).
`neo_runtime::sync_pipeline` now provides stage identifiers, Reth-style
`CommitPolicy` thresholds (`max_blocks`, `max_changes`, `max_cumulative_gas`,
`max_duration`), a provider-neutral `SyncStageCheckpointStore` seam, and
`SyncPipelineDriver`, which imports contiguous `SyncBlockBatch` values through
the shared `ImportQueue` and checkpoints the import stage when policy fires.
These close the downloader-to-import handoff; the concrete multi-stage
headers/bodies/execute/index/prune loop still remains the next large
integration step.

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
| Done | Import queue boundary with bounded concurrent `check` | Reusable preverification surface; downloader batch handoff now goes through `SyncPipelineDriver` |
| Done | Commit policy/checkpoint primitives plus import-stage driver | Tunable memory/i-o; shared crash-resume seam; ordered downloader handoff |
| P2 | Warp sync / state sync | Minutes to sync instead of hours |

---

## 3. Block Import Chain

### neo-rs (current)

`neo_runtime::BlockImport` is the canonical import trait. `BlockchainHandle`
implements it and routes to the `neo-blockchain` service loop. The reusable
`neo_runtime::BlockImportQueue` runs bounded concurrent `check` calls, then
submits the verified batch to `BlockImport::import_many` in original order.
`neo_runtime::SyncPipelineDriver` now consumes contiguous sync batches, rejects
height gaps, calls the import queue, and writes import-stage checkpoints
according to `CommitPolicy`.
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
let import_chain = Box::new(NeoHeaderVerifier)
    .and_then(Box::new(ConsensusVerifier))   // dBFT witness check
    .and_then(Box::new(StateVerifier))       // balance, nonce, conflicts
    .chain(Box::new(ClientBlockImport));     // persist to DB
```

Current status: the shared trait, bounded queue, and import-stage sync driver
exist. `neo-network::BlockDownloadBatch` converts into
`neo_runtime::SyncBlockBatch`, preserving the single ordered import path. The
remaining work is the concrete peer scheduler and broader staged pipeline.

---

## 4. Task Manager / Service Lifecycle

### neo-rs (current)

`neo-node` now has explicit daemon task supervision. Essential tasks request
node shutdown on error or unexpected exit; normal tasks report/log failures
without terminating the daemon. Metrics use bounded `task_kind` and `outcome`
labels.

```rust
pub struct DaemonTaskHandle {
    pub kind: DaemonTaskKind,
    pub criticality: DaemonTaskCriticality,
    // supervised JoinHandle plus shutdown token
}
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
runtime sync driver can feed into the import queue. The per-peer
`BlockRequestScheduler` now owns the `GetBlockByIndex` request-window policy
used by `PeerSession` (`500` blocks per request, `1000` blocks in flight,
stall rewind). The remaining work is the cross-peer stream downloader that
assigns/retries ranges across peers and yields `BlockDownloadBatch` values
directly.

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
}
// Per-peer request policy is implemented by BlockRequestScheduler; a
// cross-peer stream downloader remains the next implementation step.
```

---

## 6. Execution / VM

### neo-rs (current)

Native Rust NeoVM — no WASM. `ApplicationEngine` with per-tx child caches.
`NativeContractProvider` remains the lower-level execution seam, but
`neo-system::NodeBuilder` now makes the provider an explicit composition-root
dependency and installs the standard provider by default only after required
services validate.

### Polkadot SDK innovations

1. **WASM runtime as meta-protocol** — runtime blob in state `:code` is source
   of truth. Client is a "game console" for WASM "games".

2. **WASM instance cache** — compiled modules cached by code hash.
   `Pooling` instantiation strategy pre-allocates instances.

3. **`NativeElseWasmExecutor`** — native fast path with WASM fallback on
   version mismatch.

### Recommendations for neo-rs

1. Keep native execution (NeoVM in Rust is already fast).
2. **Done:** make `NativeContractProvider` part of `NodeBuilder` config instead
   of a hidden `install()` side effect.
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
| Import queue + concurrent verify | ★★★★ | - | ★★★ | ★★★ | Driver handoff done; peer scheduler medium |
| Stage commit policy + checkpoints | ★★★ | - | ★★★★ | ★★ | Import-stage driver done |
| Compact derive macro | ★★ | ★★★★ | - | ★★ | Small |
| Task supervision | - | - | ★★★★★ | ★★ | Done |
| BlockDownloader as Stream | ★★★ | - | ★★ | ★★★ | Boundary + per-peer scheduler done; cross-peer downloader medium |
| Essential task monitoring | - | - | ★★★★★ | ★ | Small |
| Metrics infrastructure | - | - | ★★★★ | ★★ | Medium |

---

## Implementation Order (Recommended)

1. **Essential task supervision + metrics** — implemented in `neo-node`.
2. **Typed table boundary** — implemented in `neo-storage`; compact derive is still future work.
3. **Block import queue with concurrent verification** — reusable runtime boundary implemented.
4. **Commit policy/checkpoint primitives and import driver** — implemented in `neo-runtime::sync_pipeline`.
5. **BlockDownloader as Stream** — implemented in `neo-network`; batches convert to `SyncBlockBatch`; per-peer `BlockRequestScheduler` is wired into `PeerSession`; cross-peer stream downloader remains next.
6. **Hot/Cold/Static tiering integration** (medium, big storage win)
7. **Staged sync pipeline integration** (large, biggest overall impact)

This document is a living reference — update as architecture evolves.
