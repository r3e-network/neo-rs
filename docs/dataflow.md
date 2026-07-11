# Data and Control Flow

This document explains how data and control move through the node: how the
services are wired at startup, how blocks and transactions travel from the
network into durable state, how consensus produces a block, and how JSON-RPC
reads it back. Each section pairs a diagram with a short explanation. The
focus is conceptual; it does not document private functions.

The node is a set of independent async services connected by typed channels.
There is no actor framework: each service owns a command/event loop.
`neo-system` owns reusable provider-neutral core composition; the `neo-node`
process host selects configuration, optional services, networking policy, and
task supervision. The diagrams below name the crate that owns each step.

## 1. Node startup and composition

The daemon enters through a staged application lifecycle. `neo-node` loads TOML
configuration and opens storage; `neo-system::NodeCoreBuilder` constructs the
canonical core graph; the process host supervises the returned blockchain task,
adds network/consensus/state-root policy, and finalizes the `Node`. After
composition, `RunningNode` owns startup imports, live services, and shutdown.

```mermaid
flowchart TD
    A["NodeCommand: parse CLI + load TOML config"] --> B["derive ProtocolSettings<br/>(MainNet / TestNet preset + overrides)"]
    B --> C["open Store + optional services<br/>(neo-node policy)"]
    C --> D["NodeCoreBuilder&lt;P,S,H&gt;<br/>(neo-system)"]
    D --> E["canonical StoreCache/snapshot<br/>mempool + caches + ledger context"]
    E --> F["NodeCoreLaunch:<br/>NodeCore + BlockchainTask"]
    F --> G["supervise BlockchainTask<br/>initialize genesis when local"]
    G --> H["build LocalNodeService (neo-network)<br/>+ inventory sink + BlockSource"]
    H --> I{"[consensus].enabled<br/>and key in validator set?"}
    I -- yes --> J["spawn dBFT driver (neo-consensus)"]
    I -- no --> K["relay-only"]
    J --> L["NodeCore::into_node(network)<br/>advertise durable tip"]
    K --> L
    L --> M["RunningNode: startup import<br/>then live services"]
    M --> N{"[rpc].enabled?"}
    N -- yes --> P["project RpcServices into NodeContext<br/>start jsonrpsee RPC server (neo-rpc)"]
    N -- no --> O["run until shutdown trigger"]
    P --> O
```

Two startup details matter for correctness:

- **Coherent core graph.** `NodeCoreBuilder` creates one `StoreCache` snapshot,
  mempool, header cache, ledger context, native provider, and blockchain
  service. The blockchain context and local P2P `BlockSource` share that
  canonical snapshot; state-dependent mempool admission goes through the same
  blockchain/provider boundaries.
- **Restart resume.** The durable tip is read from the store before P2P starts.
  The in-memory ledger tip and the advertised height are seeded from it, so a
  restarted node requests blocks from `tip + 1` instead of re-syncing from
  genesis (`neo-system::NodeCoreBuilder`).
- **Genesis fence.** Local initialization is a typed request/reply command. The
  daemon waits for genesis staging, backend flush, and ledger-cache publication;
  a persistence failure is returned to startup and prevents the node from
  opening live services on an uninitialized store.

## 2. Block ingestion

A block arriving from a peer is decoded by its per-peer task, buffered by
`neo-node`, and submitted through
`neo_blockchain::BlockchainHandle::submit_inventory_blocks`. Consensus-produced
blocks use `submit_inventory_block`; extensible payloads use
`submit_inventory_extensible`; startup genesis bootstrapping uses `initialize`.
The node composition code does not construct private `BlockchainCommand`
variants directly. The blockchain service then structurally and
witness-verifies blocks, runs the C# `Blockchain.Persist` pipeline, and commits
them durably. Out-of-order blocks are parked and drained when their parent
lands. The parked-block cache tracks its exact size under one lock and evicts
only the configured fraction of farthest-future candidates under pressure.
The typed inventory handle methods deliberately preserve
inventory-specific behavior (relay policy, parking, draining, and live mempool
maintenance) instead of routing peer blocks through the generic bulk-import
command.

Downloaded block batches may first pass through `neo_runtime::BlockImportQueue`:
the queue runs `BlockImport::check` with bounded concurrency and then calls
`BlockImport::import_many` in the original order. That queue is a preflight
boundary, not a second import path. `neo_network::BlockDownloader` is the
stream-shaped source boundary for peer schedulers; its `BlockDownloadBatch`
converts into `neo_runtime::SyncBlockBatch`. `SyncPipelineDriver` validates
contiguous heights, pushes those batches through the import queue, and persists
import-stage checkpoints through
`neo_runtime::sync_pipeline::{CommitPolicy, SyncStageCheckpointStore}`. The
node-level `neo_system::SyncImportPipeline` now composes the queue and durable
checkpoint provider over the same blockchain and storage handles used by the
rest of the node and is owned directly by `neo_system::Node`.
`neo_system::SyncDownloadImportDriver` seeds the driver from the canonical tip,
drains any `BlockDownloader` stream into that handle, and stops on downloader,
contiguity, partial-import, or checkpoint errors. `BlockOrigin::Sync` maps to
`ImportMode::Sync`: full verification and live artifacts are mandatory, while
a range-aware `SyncBatchCommitPolicy` may collapse canonical writes to one
durable commit. The resolved `ImportPlan` freezes observer behavior, publishes
ordered hooks/mempool updates/import events only after durability, removes
stale headers before one batch-end reverify, and falls back to per-block
durability when plugin staging is not batch-safe. Production local-ledger node startup now feeds it from the
coordinator-backed P2P downloader over live peer handles. The
per-peer `GetBlockByIndex` request window is planned by
`neo_network::BlockRequestScheduler` and sent by `PeerSession`. Cross-peer range
assignment, peer bias, and retry accounting are now owned by
`neo_network::CrossPeerBlockRangeScheduler`. The transport-agnostic
`neo_network::BlockDownloadCoordinator` composes that scheduler with
`neo_network::OrderedBlockBatchBuffer` and yields ordered `BlockDownloadBatch`
values from any `BlockRangeFetcher`. `Arc<neo_network::PeerRegistry>` now
implements that fetcher by resolving the assigned peer handle, sending
`GetBlockByIndex`, and collecting the matching block frames into a batch; the
same registry records advertised peer heights for downloader snapshots and is
registered by the node composition root. Local-ledger node startup disables
legacy automatic per-peer block requests and runs the coordinator-driven
downloader/import task as the production P2P range-sync owner.
The canonical execution/persist path remains the `neo-blockchain` service loop.

```mermaid
sequenceDiagram
    participant Peer
    participant RN as RemoteNode (neo-network)
    participant Fwd as Inventory forwarder (neo-node)
    participant BC as BlockchainService (neo-blockchain)
    participant NP as native_persist pipeline
    participant AE as ApplicationEngine (neo-execution)
    participant Guard as LocalReplayGuard (neo-node)
    participant State as StateService / persistent indexer
    participant Store as DataCache / StoreCache (neo-storage)

    Peer->>RN: block message
    RN->>Fwd: InboundInventory::Block
    opt batch preflight
        Fwd->>Fwd: BlockImportQueue::push_blocks<br/>(bounded check concurrency)
    end
    Fwd->>BC: submit_inventory_blocks / submit_inventory_block
    Note over BC: index == height + 1?<br/>else park / ignore
    BC->>BC: structural checks (version, tx count,<br/>merkle root, no duplicate tx)
    BC->>Store: load prev TrimmedBlock
    BC->>BC: verify consensus witness vs<br/>prev.NextConsensus (3-GAS cap)
    BC->>NP: stage_block_natives_with_resources(block)
    NP->>AE: OnPersist engine (+ native init at activation)
    loop each transaction
        AE->>AE: Application engine, gas = tx.SystemFee
        Note over AE: HALT commits per-tx cache; FAULT discards
    end
    NP->>AE: PostPersist engine
    NP->>Store: stage all writes in child cache, commit on success
    opt independent pre-commit store configured
        BC->>Guard: write + fsync recovery marker
        BC->>State: pre-commit update + durable fence
    end
    BC->>Store: commit_to_store() -> Result<br/>flush to durable backend
    alt commit failed
        Store-->>BC: error; discard root overlay
        BC->>BC: rewind staged batch tip; publish nothing
    else commit succeeded
        BC->>Guard: remove marker + sync directory
        BC->>BC: block_committed observers in height order
    end
    BC->>BC: mempool.block_persisted (drop mined + conflicts)
    BC-->>Fwd: RuntimeEvent::Imported { height }
```

Ownership by step:

| Step | Owner crate |
|------|-------------|
| Decode wire frame, per-peer state machine | neo-network |
| Sequencing, structural + witness verification | neo-blockchain |
| Native OnPersist / PostPersist + tx execution | neo-blockchain → neo-execution + neo-native-contracts |
| Per-block atomic staging, durable commit | neo-storage |
| Mempool eviction of mined/conflicting tx | neo-mempool |

The whole `Persist` sequence is staged in a child `DataCache` and merged into
the shared snapshot only when every stage succeeds; a fault leaves no partial
block state. Locally produced (consensus) blocks set `pre_verified = true` and
skip the peer-witness check.

## 3. Transaction lifecycle

A client submits a raw transaction over RPC. The RPC layer hands it to the
blockchain service, which runs the full `Transaction.Verify` pipeline through
the mempool. On a fresh accept, the daemon re-announces it to peers so it
propagates and is eventually included in a block.

```mermaid
sequenceDiagram
    participant Client
    participant RPC as RpcServer (neo-rpc)
    participant BC as BlockchainService (neo-blockchain)
    participant MP as MemoryPool (neo-mempool)
    participant Net as LocalNode (neo-network)
    participant Peers

    Client->>RPC: sendrawtransaction(base64 tx)
    RPC->>BC: add_transaction(tx)
    BC->>MP: try_add(tx, store snapshot, settings)
    Note over MP: state-independent: size, strict script,<br/>standard sig/multisig fast path
    Note over MP: state-dependent: validity window, blocked accounts,<br/>conflicts, sender GAS balance, attribute + witness fees
    alt VerifyResult::Succeed
        MP-->>BC: admitted
        BC-->>RPC: { hash }
        RPC-->>Client: tx hash
        BC->>Net: broadcast Inv(Transaction, hash)
        Net->>Peers: Inv → peers pull via GetData
    else rejected
        MP-->>BC: VerifyResult (Expired, PolicyFail, HasConflicts, ...)
        BC-->>RPC: error
        RPC-->>Client: error
    end
    Note over MP,BC: later: block_persisted drops the tx<br/>from the pool when mined
```

Peer-relayed transactions take the same path: the inventory forwarder calls
`add_transaction`, and a fresh `Succeed` triggers the same `Inv` re-announce
(`neo-node/src/node.rs`). Verification logic lives in
`neo-mempool/src/verification.rs` and ports C# `Transaction.VerifyStateIndependent`
followed by `VerifyStateDependent`.

## 4. dBFT consensus round

When `[consensus]` is enabled and this node's key is in the validator set, the
dBFT driver participates in single-block rounds. The primary proposes a block;
backups respond; once `M = N - (N-1)/3` commits are collected the block is
assembled and handed to the blockchain service as a pre-verified block.

```mermaid
sequenceDiagram
    participant Primary
    participant Backups
    participant BC as BlockchainService

    Note over Primary,Backups: Reset for block index; primary = (index) mod N
    Primary->>Primary: collect tx from mempool (RequestTransactions)
    Primary->>Backups: PrepareRequest (proposed tx, timestamp, nonce)
    Backups->>Backups: validate proposal
    Backups->>Primary: PrepareResponse
    Note over Primary,Backups: M preparations reached
    Primary->>Backups: Commit (signature over block header)
    Backups->>Primary: Commit
    Note over Primary,Backups: M commits reached
    Primary->>BC: assembled block (pre_verified)
    BC->>BC: persist (section 2), advance tip
```

```mermaid
flowchart LR
    P[Primary state] -->|PrepareRequest sent| R[Awaiting responses]
    R -->|M preparations| C[Committing]
    C -->|M commits| B[Block produced]
    R -.->|timeout / TxNotFound| V[ChangeView]
    P -.->|timeout| V
    V -->|new view, next primary| P
```

Messages travel as `ExtensiblePayload` over P2P (category `dBFT`). The driver
decodes and authenticates inbound payloads against the validator set before
processing; payloads are deduplicated by `ExtensiblePayload.Hash` only after
their witness verifies, so a forged-witness payload cannot suppress the genuine
one (`neo-consensus/src/service/core.rs`). On timeout (or missing proposed
transactions) a backup requests a view change, rotating to the next primary.

## 5. RPC request handling

JSON-RPC requests are served by a jsonrpsee server. A request is parsed, the
method is resolved (case-insensitive) against a registry, and the handler reads
from the ledger snapshot, MPT/state store, or mempool to build the response.

```mermaid
sequenceDiagram
    participant Client
    participant JR as jsonrpsee transport (neo-rpc)
    participant D as dispatch (resolve handler)
    participant H as handler group
    participant N as Arc<NodeContext> (neo-rpc)

    Client->>JR: HTTP/WS JSON-RPC request
    JR->>D: method + params
    D->>D: lookup handler (registry), reject disabled
    D->>H: invoke (panic-safe)
    alt read methods
        H->>N: store snapshot / mempool / state store
        N-->>H: block, tx, state root, balances, ...
    else relay methods (sendrawtransaction, submitblock)
        H->>N: blockchain.add_transaction / import_block
    end
    H-->>JR: serde_json result
    JR-->>Client: JSON-RPC response
```

Handlers are grouped by domain and registered at startup
(`neo-node::node::rpc_runtime` calls `register_handlers` for blockchain, node, state,
wallet, utilities, smart-contract, plus the optional application-logs,
tokens-tracker, and oracle groups). The dispatcher catches handler panics and
applies the configured `UnhandledExceptionPolicy`
(`neo-rpc::server::dispatch`). `NodeContext` carries core typed handles plus an
immutable `RpcServices<S>` bundle with named optional state, indexer,
application-log, and token-tracker services. Read handlers query those handles;
relay handlers forward to the blockchain service over its command channel.

| Handler group | Example methods | Reads from |
|---------------|-----------------|------------|
| blockchain | getblock, getblockcount, getrawtransaction, getcontractstate | ledger snapshot |
| node | getversion, getpeers, getconnectioncount | network handle |
| state | getstateroot, getstateheight, getproof, getstate, findstates | state store / MPT |
| state / mempool | getrawmempool | mempool |
| smart-contract | invokefunction, invokescript, calculatenetworkfee | snapshot + ApplicationEngine |
| relay | sendrawtransaction, submitblock | blockchain service |

## 6. State and storage

All execution writes go to an in-memory `DataCache` overlay over the `Store`
backend. A block's writes are staged in a child cache, committed into the
shared snapshot when the block succeeds, and flushed to the durable backend by
`commit_to_store()`. The Merkle-Patricia-Trie state root is maintained by the
state service over the block change set and exposed through the state RPCs.

```mermaid
flowchart TD
    subgraph Execution
        AE["ApplicationEngine writes"] --> Child["per-tx / per-block child DataCache"]
    end
    Child -->|commit on success| Snap["shared DataCache snapshot"]
    Snap -->|commit_to_store| SC["StoreCache"]
    SC -->|flush| Store["Store backend<br/>MDBX / RocksDB / in-memory (neo-storage)"]

    Snap -.->|block change set| MPT["MptStore (neo-state-service)<br/>Trie over change set → state root"]
    MPT --> SS["StateStore root records<br/>by index / by hash"]
    SS -->|getstateroot / getstateheight| RPC1["state RPC"]
    MPT -->|getproof / getstate / findstates| RPC2["proof RPC"]
```

Key points:

- **Overlay model.** A `DataCache` records adds/changes/deletes over a parent
  store; `commit()` merges a child into its parent atomically, and dropping a
  child discards its writes. This is the per-block atomicity mechanism
  (`neo-blockchain/src/native_persist.rs`).
- **Durability.** `commit_to_store()` is fallible and flushes the snapshot's
  tracked writes through `StoreCache::try_commit_durable()` to the configured
  backend. Every canonical backend must implement the atomic durable borrowed-
  overlay capability; the writer rejects unsupported stores instead of using
  an unsafe commit-then-flush fallback. MDBX uses one atomic write transaction,
  while RocksDB bypasses fast-sync buffering with a WAL-synchronous batch and
  first persists any earlier WAL-disabled prefix.
  A failure discards the uncommitted root overlay; bulk paths also rewind their
  staged in-memory tip. Post-commit observers, mempool maintenance, and import
  events run only after this fence succeeds. MDBX is the production default,
  RocksDB remains a supported fallback, and the in-memory backend does not
  persist across restarts.
- **Cross-store recovery.** StateService and a persistent indexer do not share a
  transaction with the canonical Ledger store. The node writes and fsyncs
  `.neo-local-replay-poisoned` before either pre-commit store can publish, then
  durably fences both observer backends before committing Ledger. A persistent
  observer mutation or fence failure rejects the block. Canonical success
  removes the marker and syncs its directory. A crash, deferred-hook
  failure, or Ledger commit failure leaves the marker for startup to reject.
  Startup also rejects an MPT height that does not match the canonical height,
  including the genesis edge case where the chain is uninitialized but
  StateService already has root `0`. Operators must restore matching stores;
  pruning-mode MPT data is never guessed or rolled back automatically.
  ApplicationLogs and TokensTracker persist post-canonical and do not arm this
  marker. Even without an auxiliary-store hazard, canonical durability failure
  cancels the node because the process must not continue on an indeterminate
  storage result.
- **Storage byte boundary.** `neo-storage` exposes `Store`,
  `RawReadOnlyStore`, `ReadOnlyStoreGeneric`, `DataCache`, and `StoreFactory`
  over existing raw bytes. `StorageKey` still encodes with `to_array()` and
  `StorageItem` with `to_value()`. No typed table/codec API is currently
  exported; any future one must preserve these C#-compatible bytes.
- **Ledger provider boundary.** `neo-blockchain` exposes `BlockProvider` and
  `TxProvider` capability traits. `StorageLedgerProviderFactory` creates hot
  providers over native Ledger records; `HotColdLedgerProviderFactory` composes
  hot reads with any cold provider that implements the same capability traits.
  Static archive writes are explicit integration work, not an implicit side
  effect of block import.
- **State read boundary.** `neo-state-service` exposes `MptReadSnapshot`,
  `MptStore`, `StateStore`, and `StateStoreLookup`. RPC proof/state paths use
  concrete immutable `MptReadSnapshot` values. A general
  `StateProviderFactory`/`StateView` capability is not exported yet.
- **State root.** The MPT root is computed from a block's storage change set and
  stored under the C# `StateRoot` wire layout (`neo-state-service/src/mpt_store.rs`),
  with per-block records by index and by hash. `getstateroot`/`getstateheight`
  serve from the `StateStore` verification cache and fall back to the live
  `MptStore`; `getproof`/`getstate`/`findstates` walk the persisted MPT, and
  return a clean `UnsupportedState` error if no MPT backend is registered
  (`neo-rpc/src/server/rpc_server_state.rs`).
