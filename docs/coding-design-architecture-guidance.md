# Coding, Design, and Architecture Guidance

This guide is the project-wide contract for writing neo-rs code that reads as
high-level Neo domain logic while keeping protocol, storage, RPC, VM, network,
and runtime mechanics in lower layers. It is inspired by compact Neo tooling
codebases such as <https://github.com/neoburger/TEE>, where entrypoints read as
business workflows rather than implementation detail.

Do not copy that style mechanically. neo-rs is a Rust full node with async
services, hot paths, durable storage, and Neo C# parity constraints. The goal is
to make top-level code short and readable without hiding correctness,
side-effects, or cost.

New code and meaningful refactors should follow this guide unless a local
module has a stronger protocol, performance, or compatibility rule.

## External Rust Baseline

This guide is the neo-rs project contract. It is supported by the Rust API
Guidelines, the rust-analyzer style guide, and Apollo's Rust best-practices
handbook. At the time this rule was added, the Apollo handbook resolved to git
revision `8425b336d368edaddbab8a5339030c677d63dc5d`.

Use those references as Rust idiom baselines, then apply the stricter neo-rs
layering, protocol, and performance rules below. The most important imported
rules are:

- Borrow before cloning. Prefer `&str`, `&[T]`, and borrowed iterators when the
  callee only reads data.
- Pass small `Copy` domain values by value when that is clearer and cheap.
- Avoid eager fallback allocation; prefer `_else` variants such as
  `ok_or_else`, `unwrap_or_else`, and `map_or_else` when fallback construction
  is non-trivial.
- Do not create intermediate collections in sync, storage, VM, RPC, or MPT
  paths unless the collection itself has domain meaning.
- Prefer `Result` over production panics. `unwrap`, `expect`, `panic!`,
  `todo!`, and `unimplemented!` are test-only unless an impossible invariant is
  documented.
- Library crates expose typed errors or `CoreError`; `anyhow` belongs in
  binaries, CLI orchestration, tools, and tests.
- Static dispatch is the default for known and hot collaborators; use
  `dyn Trait` only when runtime type erasure is the actual design.
- Prefer maintained Rust crates for commodity algorithms and data structures.
  Keep local wrappers only for Neo protocol byte layout, C# compatibility,
  error mapping, framing, or storage semantics, and protect those wrappers with
  parity tests before replacing or simplifying them.
- Performance work starts from measurement: release builds, focused benchmarks,
  flamegraphs, sync-speed probes, or concrete allocation evidence.
- Tests are living API documentation: descriptive names, one behavior per test,
  and explicit error-path coverage.
- Public APIs need rustdoc that states contracts, side effects, errors, panics,
  safety, and examples when practical.
- Comments explain why, not what. Use named comments such as `SAFETY:` and
  `CONTEXT:` for non-obvious invariants. Bare TODOs should become tracked
  issues or be removed.

## Core Contract

- Top-level code reads as ordered Neo domain intent.
- Lower layers own mechanics and expose named operations upward.
- Each abstraction has one owner, one vocabulary, and one test surface.
- Rust types carry protocol meaning across boundaries.
- Known hot-path collaborators use concrete types, generics, or associated
  types by default.
- `dyn Trait` is reserved for named runtime boundaries.
- Chaining is allowed only when it clarifies the business sequence.
- Consensus, persistence, network I/O, shutdown, and fallback decisions stay
  explicit and auditable.
- Runtime data stays out of git; the repository stores source, docs, scripts,
  and small deterministic fixtures only.

A change can be rejected even if it compiles when it forces readers to
understand RocksDB keys, JSON-RPC transport, VM stack encoding, MPT node layout,
and node orchestration in the same function.

## Blockchain Node Priorities

neo-rs is a blockchain node, not a generic server. The engineering priority
order is:

1. Determinism.
2. Protocol correctness through types and tests.
3. Clear module and trait boundaries.
4. Performance after measurement.

Performance work must never weaken deterministic execution, replayability, or
Neo N3 v3.10.1 protocol parity. A faster node that can diverge from mainnet
state is a broken node.

Practical node rules:

- Use existing crates for generic cryptography, compression, Base58/Base64/hex,
  LRU/cache structures, byte buffers, and hash implementations whenever their
  semantics match the required behavior.
- Do not swap consensus-critical codecs, Merkle roots, MPT state roots, Bloom
  filter wire behavior, native-contract serialization, or Neo binary IO to a
  generic crate unless byte-for-byte parity is proven against Neo C# / mainnet
  replay fixtures. The local adapter should be thin, documented, and tested;
  the commodity algorithm beneath it should still come from a maintained crate
  when possible.
- Do not use nondeterministic iteration order in consensus, execution, state
  transition, serialization, or hash/root construction. Prefer ordered maps,
  explicit sorting, or C# parity order.
- Do not use floating point in consensus-critical calculations.
- Do not use wall-clock time inside deterministic execution. Network timeouts,
  metrics, and operator logs are outside the state transition.
- Bound inbound message sizes, recursive decoding depth, transaction/script
  cost, and RPC request work before allocating large buffers.
- Keep `unsafe` out of ordinary node logic, but allow it in measured hot paths
  when safe Rust cannot reach the target without extra bounds checks, virtual
  dispatch, copies, or FFI overhead. Unsafe code must be isolated behind a safe
  public API, documented with `SAFETY:` invariants, covered by parity tests or
  fuzz/property tests, and justified with benchmark or profiling evidence. Do
  not weaken deterministic execution, replayability, or error handling to win a
  microbenchmark.
- Treat `unwrap`, `expect`, and `panic!` in consensus, execution, sync, storage,
  and RPC parsing as stop-the-node risks unless they prove an impossible
  invariant.
- Mainnet replay, state-root parity, and reference RPC checks are authoritative
  correctness evidence. Narrow unit tests are necessary but not sufficient for
  protocol completion claims.

## Commodity Primitive Policy

Prefer maintained Rust crates for commodity algorithms. Local neo-rs code should
own Neo protocol semantics, not reimplement standard algorithms under familiar
names. A wrapper is justified only when it preserves one of these contracts:

- Neo/C# byte layout, endian order, wire framing, or error mapping.
- hardfork-gated behavior or native-contract compatibility.
- state-root, Merkle-root, address, wallet, or storage-key parity.
- measured hot-path adaptation behind a safe API and parity tests.

“Official library” means different things in different areas. For protocol
bytes, the authoritative reference is Neo C# / mainnet replay parity. For
commodity math or data structures, prefer maintained crates such as RustCrypto,
`murmur3`, `bitvec`, `hex`, `bs58`, and `base64`; keep neo-rs code as a thin
adapter that names the Neo contract being protected. Do not replace a local
consensus primitive merely because a generic crate has the same algorithm name.

Use this matrix when reviewing `neo-crypto`, `neo-io`, storage, wallet, and
network code:

| Area | Upstream crate should own | Local neo-rs code should own |
| --- | --- | --- |
| Hex, Base58, Base64 | Encoding/decoding algorithms (`hex`, `bs58`, `base64`) | Neo address payload versioning, checksum error mapping, and .NET-compatible strict/lenient behavior |
| LZ4 and compression | Compression codec implementation (`lz4_flex` or equivalent) | Neo package framing, size limits, checksum policy, and DoS guards |
| Binary readers/writers | byte buffers, endian helpers, and `std::io` traits | Neo var-int, `ISerializable`-style contracts, max-size checks, and C# parity errors |
| Cache containers | LRU/concurrent cache mechanics (`lru`, `dashmap`, future `moka` only with reason) | state-tracking caches such as `DataCache`, MPT overlays, transaction visibility, and commit semantics |
| Murmur | Murmur3 hash function | Neo Bloom-filter seed schedule and error mapping |
| Bloom filters | bit storage primitives and hash functions | Neo wire bit layout, Murmur seed schedule, and network payload compatibility |
| Signatures | curve math and verification (`p256`, `secp256k1`/`k256`, `ed25519-dalek`, `blst`) | raw `r||s` framing, low-s/C# parity, NeoFS prefixes, hardfork gates, and native-contract error mapping |
| BIP-32/BIP-39 | mnemonic and derivation primitives when the crate supports the required curve and serialization | Neo wallet defaults, NEP-6 account model, P-256 behavior, path policy, and zeroization boundaries |
| Merkle tree | reusable proof helpers only if custom hash/duplication/layout is exact | Neo block/MerkleBlock root bytes: odd-leaf duplication, `Hash256(left || right)`, little-endian `UInt256`, and trim/proof shape |
| MPT trie | reference ideas and low-level data structures only | Neo C# MPT node types, serialization, empty-node behavior, hash domain, proof shape, and state-root parity |

Do not introduce project-local types named like generic utilities (`Hex`,
`Base58`, `BinaryWriter`, `MemoryReader`, `BloomFilter`, etc.) unless the type
name represents a real Neo protocol contract. If a thin adapter remains, its
module rustdoc must state which upstream crate does the commodity work and which
Neo-specific behavior the adapter protects.

## Layer Rules

Each layer should speak one vocabulary and hide the vocabulary below it.

| Layer | Should expose | Should hide |
| --- | --- | --- |
| Application | `NodeCommand`, opened runtime, requested operating mode | task handles, stores, command loops, protocol mechanics |
| Plugin/RPC boundary | typed request/response and adapter capabilities | core construction and service ownership |
| Composition | `NodeCoreBuilder`, `NodeCoreLaunch`, `Node`, sync workflows | protocol rules and application policy |
| Node service | root-level handles, providers, outcomes, protocol values | command-loop state and source module layout |
| Domain service | import, execution, native, state, and admission contracts | CLI, RPC transport, and process lifecycle |
| Protocol | blocks, transactions, witnesses, dBFT messages | stores, HTTP, and process startup |
| Infrastructure | precise codec, storage, VM, config, and crypto capabilities | node workflow and business policy |
| Foundation | domain value types | services and mechanical adapters |

Practical rules:

1. Higher layers may call lower-layer verbs. Lower layers must not import
   higher-layer crates to reuse orchestration helpers.
2. A function should stay at one abstraction level. Node startup should not
   parse MPT nodes; RPC handlers should not decide peer-sync policy.
3. Move detail downward into the layer that owns it, not sideways into broad
   `utils`, `helpers`, or `misc` modules.
4. Keep C# parity rules close to the code that implements them.
5. Return named outcomes at workflow boundaries, such as `FastSyncReport`,
   `BlockPersistOutcome`, `StateRootCommitReport`, or
   `RpcOnlyRuntimeHandle`.

## Crate and Module Rustdoc

Every crate root (`src/lib.rs` or binary `src/main.rs`) and every module entry
file (`src/**/mod.rs`) must start with an inner rustdoc block (`//!`). The block
is the reader's map for that layer, so keep it short, current, and specific.

Use this shape unless a protocol module needs a stronger local format:

```rust
//! # crate-or-module-name
//!
//! One paragraph explaining what this crate/module owns.
//!
//! ## Boundary
//!
//! State the layer, the main dependencies it may call downward, and the
//! responsibilities it deliberately does not own.
//!
//! ## Contents
//!
//! - `child_module`: what it owns.
//! - `important_type`: why callers use it.
```

Rules:

- Crate docs explain the crate's role in the workspace architecture, not every
  public item.
- Module docs explain the module's local responsibility and boundary.
- Mention C# parity, deterministic bytes, storage layout, or security
  invariants only where the module directly owns that contract.
- Keep examples small and compileable when practical; otherwise mark them
  `ignore` and explain why they are illustrative.
- Update rustdoc when moving files, changing crate layers, changing storage
  backends, or replacing a concrete service with a trait boundary.
- Do not leave stale names such as removed crates, old versions, or old module
  paths in top-level crate/module comments.
- When adding a provider, queue, factory, table, or service boundary, document
  both the capability it exposes and the lower-level mechanics it intentionally
  hides. Call out whether it preserves C# byte formats, store layout, or
  execution ordering.

## Reth/Polkadot Layering Priorities for Neo

Use reth and Polkadot as architectural references, not as templates to copy
wholesale. Neo has dBFT immediate finality and native contracts, so the highest
value patterns are the ones that simplify the import path and service
supervision without importing fork-choice or meta-runtime complexity.

Priority order for crate refactors:

1. **One canonical block-import path.** Consensus, sync, fast sync, and RPC
   submission should depend on `neo_runtime::BlockImport` and receive
   `BlockImportOutcome` / `ImportedTip` rather than reaching into
   `neo_blockchain::BlockchainCommand`. `neo-blockchain` owns the concrete
   validation, execution, native-persist, state-root, and durable-store
   implementation behind that trait. Use `neo_runtime::BlockImportQueue` for
   bounded concurrent preverification, but keep ordered import in
   `BlockImport::import_many`. Use `neo_system::SyncImportPipeline` as the
   node-composed handle that binds the canonical blockchain importer, bounded
   queue, shared store-backed sync checkpoints, and import-stage commit policy;
   keep it as one explicit `Node` field so downloader and service consumers
   clone the same `Arc` instead of composing parallel queues.
   Downloader code should expose `neo_network::BlockDownloader` streams and use
   `neo_system::SyncDownloadImportDriver` to convert downloaded batches into
   ordered `neo_runtime::SyncPipelineDriver` submissions. Stage flushing and
   crash-resume markers belong in
   `neo_runtime::sync_pipeline::{CommitPolicy, SyncStageCheckpointStore}`
   instead of ad hoc thresholds inside service loops.
   Treat the durable store fence as fallible: do not publish committed events,
   run post-commit observers, or advance externally visible in-memory state
   until it succeeds. On failure, discard the canonical overlay and rewind any
   batch-local tip before returning the error.
   Canonical stores must implement the atomic durable-overlay capability;
   never substitute commit-then-flush because a failed flush cannot roll back
   the already-applied overlay. Never claim atomicity across independently
   committed stores. StateService and a persistent indexer may persist before
   the canonical Ledger fence. Write and fsync an operator-visible marker
   *before* entering either observer, durably fence both before Ledger, and
   fail the canonical write if either persistent observer cannot mutate or
   fence its data. Clear the marker only after Ledger succeeds. A crash or failure leaves
   the marker for startup to reject until store heights match. ApplicationLogs
   and TokensTracker stage pre-commit data but persist only post-canonical, so
   they must not pay the marker fsync cost. Do not call MPT rollback as a
   generic repair because pruning may already have removed required nodes.
   A self-reconciling immutable mirror may fence before canonical storage
   without the poison marker only when startup validates the shared prefix and
   deterministically truncates every cold-ahead suffix. Do not apply this
   exception to mutable observer stores.
   Cancel the node on every canonical durability failure and stop the active
   writer command immediately.
   Per-peer request-window decisions belong in
   `neo_network::BlockRequestScheduler`; session code should only serialize and
   send the planned wire request. Cross-peer range assignment, peer bias, and
   retry accounting belong in `neo_network::CrossPeerBlockRangeScheduler` so the
   downloader policy stays independent from wire transport. Use
   `neo_network::BlockDownloadCoordinator` to compose that scheduler with
   `neo_network::OrderedBlockBatchBuffer` and a transport-specific
   `neo_network::BlockRangeFetcher`; production P2P range fetching goes
   through the connected-peer registry/remote-node handle, uses the registry's
   advertised-height snapshots for range assignment, and leaves ordered release
   to the coordinator before batches reach the runtime import queue.
2. **One reorg-aware chain event stream.** Indexers, RPC application logs,
   token trackers, oracle services, and plugins should derive from a single
   bounded stream of chain outcomes. Because Neo committed blocks are final,
   revert handling should stay explicit but normally cold.
3. **One task-supervision layer.** Composition code should distinguish
   essential node tasks from normal background tasks. Essential task failure
   should trigger graceful node shutdown; normal task failure should be logged,
   metered, and restarted or disabled according to policy.
4. **One provider/factory pattern for reads.** Storage-backed read APIs should
   expose capability traits and factories rather than leaking concrete caches
   or backend handles upward. The live ledger pattern is
   `BlockProvider`/`TxProvider` plus `LedgerProviderFactory`; state reads
   currently use concrete immutable `MptReadSnapshot` values, so add a state
   factory only when a real second implementation or consumer boundary needs it.
5. **Typed tables may only adapt existing bytes.** The current live storage API
   is `Store`/`RawReadOnlyStore`/`ReadOnlyStoreGeneric` over
   `StorageKey`/`StorageItem`; no typed table codec is exported. If one is added,
   it must adapt existing persisted bytes and must not invent a new consensus
   serialization format.

Do not copy reth's heavy memory-chain reorg overlay or Polkadot's Wasm
meta-runtime into Neo just for symmetry. Keep the part that matters here:
explicit contracts in domain API crates, concrete implementations in node
service crates, and composition-only wiring above them.

## High-Level Flow Rules

Top-level workflows should read like a Neo operation:

```rust
NodeCommand::from_cli(args)?
    .open_runtime()
    .await?
    .run_requested_mode()
    .await
```

For longer workflows, use named domain steps:

```rust
let report = FastSyncWorkflow::for_network(network)
    .using_builtin_source()
    .ensure_local_package()
    .verify_package()
    .import_chain_acc()
    .restore_live_store_mode()
    .finish()
    .await?;
```

Rules for workflow facades:

- The facade owns one use case, not an entire subsystem.
- Each method name is a real domain transition, not generic plumbing.
- Side effects are visible in method names or result types.
- Errors include the domain operation that failed.
- Important branches use named `let`, `if`, or `match` blocks.
- The terminal method makes completion obvious: `finish`, `start`, `serve`,
  `submit`, `persist`, or a more specific domain verb.
- The facade returns a named report or handle rather than unrelated primitives.
- The lower mechanics remain independently testable.
- A staged builder should make invalid ownership graphs unrepresentable. For
  example, `NodeCore::into_node(network)` consumes the core assembled by
  `NodeCoreBuilder`; callers cannot finalize a node with a different store,
  mempool, cache set, or native provider.

Node-service callers import stable capabilities from crate roots. Do not make
source organization part of the API (`neo_blockchain::service::*`,
`neo_network::wire::*`, or `neo_network::proto::*`). Prefer
`neo_blockchain::BlockchainHandle`, `neo_network::NetworkHandle`, and root-level
wire/protocol values. Implementation modules may change without forcing
cross-crate migrations.

Break the chain when the code performs a decision reviewers must audit:

```rust
let manifest = package.verify_manifest().await?;

match manifest.import_plan(target_height)? {
    ImportPlan::AlreadyCurrent(report) => return Ok(report),
    ImportPlan::ImportRange(range) => importer.import_range(range).await?,
}
```

Avoid chains that only compress detail:

```rust
run().handle().process().finalize().await?;
```

The bad version has chaining syntax but does not reveal the node operation.

## Chaining Rules

Use fluent or chained APIs when:

- The steps are naturally ordered.
- Each method consumes or returns the same workflow, builder, command, or result
  wrapper.
- Every verb has local domain meaning.
- Error handling remains explicit through `CoreResult`, typed errors, or `?`.
- The chain is short enough to scan without hiding important branches.

Avoid chaining when:

- The workflow has consensus-critical checks, retries, fallback behavior,
  partial imports, shutdown, or irreversible writes.
- Intermediate values explain protocol state and deserve names.
- Method names become `process`, `handle`, `run`, `do_step`, or `finalize`
  without domain context.
- The API would need avoidable allocation, dynamic dispatch, or lifetime
  gymnastics.
- The chain mixes configuration, validation, execution, persistence, and
  reporting with different failure behavior.

A chain longer than roughly six business steps should usually become named
phases or a workflow type with private helpers.

## Generics and `dyn Trait`

This is not a "generics everywhere" rule. It is a boundary-honesty rule.

Use concrete types, generics, or associated types when the collaborator is known
at compile time, especially in block sync, state-root, MPT, storage, VM, and
networking paths. Prefer a closed enum when configuration selects among the
implementations shipped by this workspace (`RuntimeStore`, local/remote ledger
sources, native-contract catalogs, signer backends). Reserve `dyn Trait` for a
genuinely open-ended external extension boundary whose implementor set cannot
be represented by a workspace-owned enum. No current node hot path or service
composition boundary requires one.

| Situation | Prefer | Avoid |
| --- | --- | --- |
| Hot block import, state-root, MPT, VM, storage, or networking loop | concrete type, generic, or associated type | allocation-heavy trait object |
| One method needs type flexibility | method-level generic | adding type parameters to a whole public type |
| A public facade has a stable compile-time collaborator | struct generic or associated type | `Box<dyn Trait>` by default |
| Dependency is selected at runtime from a closed set | named enum with typed variants | erased service locator or scattered downcasts |
| Dependency is an open-ended external extension | one documented `dyn Trait` boundary outside hot loops | propagating erasure into protocol code |
| Repeated payload crosses a layer | named struct or enum | raw tuple, `serde_json::Value`, raw stack item, or byte map |
| Unknown escape hatch or convenience-only dependency | narrower domain type or trait | `dyn Any`, broad service locator traits |

Rules:

- Do not add generics to make signatures look more professional. Add them when
  they encode a stable compile-time relationship, improve correctness, remove
  hot-path dispatch cost, or make ownership clearer.
- Do not replace a clear runtime boundary with unrelated type parameters;
  prefer a closed enum at the composition root when the implementation set is
  workspace-owned.
- Prefer method-level generics when only one operation needs flexibility.
- Prefer associated types when a trait has one natural output, key, or backing
  store type.
- Bound generics with the smallest trait that expresses the operation.
- Avoid `Arc<dyn Trait>` as the default dependency shape. Use it only when
  open-ended shared runtime polymorphism is actually required and a closed enum
  cannot express the supported implementations.
- Do not use `TypeId`/`Any` maps as service composition. Put each supported
  service in a named typed field and pass the smallest typed bundle to its
  consumers.
- For statically dispatched async traits, return `impl Future + Send` (or use
  an associated future type). Do not use `async_trait` in protocol or node hot
  paths because it boxes a future for every call.
- If a trait object remains in a hot path, document the runtime boundary and
  verify the cost is acceptable.
- Keep trait definitions close to the API crate or module that owns the
  contract. Implementations belong in lower implementation modules or crates.
  Higher layers should depend on the contract, not a concrete backend.
- Use sealed traits for public extension points only when downstream
  implementations would break protocol invariants.

## Domain Type and State-Machine Rules

Primitive obsession creates protocol bugs. Repeated protocol values should have
domain names and domain methods.

- Use newtypes or existing domain types for block hashes, state roots, script
  hashes, addresses, heights, nonces, storage keys, and network magic instead
  of raw arrays, bytes, strings, or integers at public boundaries.
- Keep conversions explicit at transport and storage edges.
- Consider typestate for irreversible protocol workflows when it prevents an
  invalid transition without making ordinary code hard to read. Good candidates
  are block validation/import phases such as unverified, verified, executed,
  and committed.
- Do not introduce typestate just to look sophisticated. Runtime matches and
  named result enums are better when branches are dynamic, persisted, or
  operator-controlled.

## Concurrency Rules

Separate I/O concurrency from CPU work.

- Network, RPC, timers, and disk coordination use Tokio async tasks.
- Signature verification, VM execution batches, trie hashing, and expensive
  state-root work must not block async executor threads. Use dedicated worker
  threads, `spawn_blocking`, or an existing CPU worker path.
- Prefer actor/message-passing ownership for node components. A service should
  own its mutable state and receive commands over bounded channels.
- Use bounded channels for network, sync, mempool, and execution queues. Define
  backpressure behavior explicitly: wait, reject, evict, or drop with metrics.
- Never hold a lock guard across `.await`.
- Use locks for small critical sections only. Hot shared reads should prefer
  snapshots, sharding, or read-optimized structures when measurement shows
  contention.

## Ownership and Allocation Rules

Ownership choices should tell reviewers whether data is being observed,
transformed, cached, or transferred.

- Use `&T`, `&str`, `&[T]`, and iterators for read-only APIs.
- Take ownership only when the operation consumes the value, stores it, mutates
  an owned builder, snapshots state, crosses an async task/thread boundary, or
  matches an external API that requires ownership.
- Clone late and intentionally. `Arc::clone` and `Rc::clone` are acceptable
  sharing signals; cloning large `Vec`, `HashMap`, byte buffers, blocks,
  transactions, witnesses, or trie nodes in loops requires a measured reason.
- Prefer `.copied()` or `.cloned()` at the iterator boundary over per-item
  closure clones when collecting owned values is actually required.
- Prefer lazy fallbacks when the fallback allocates or performs work:
  `ok_or_else`, `unwrap_or_else`, `map_or_else`, and `or_else`.
- Use `Cow` only when an API naturally accepts either borrowed or owned data.
  Do not introduce `Cow` to hide unclear ownership.
- Avoid passing large structs by value across hot paths. Use references or
  smaller named domain values unless ownership transfer is the point.
- Avoid `#[inline]` and custom allocation tricks until a benchmark or profile
  shows the compiler needs help.

In node sync and state paths, allocation discipline is a correctness-adjacent
operational concern: avoidable clones and intermediate buffers lower achievable
BPS and make memory pressure less predictable.

## Typed Boundary Rules

Decode unstructured values at the boundary that understands their meaning.

- Do not let `serde_json::Value`, raw stack items, unlabeled byte vectors, or
  loosely typed maps escape a boundary that can decode them into a Neo type.
- Use named structs and enums for repeated protocol shapes, even when they start
  with two fields.
- Public workflow APIs should pass and return values with Neo meaning:
  `FastSyncManifest`, `ImportPlan`, `CandidateVote`, `ContractStorageKey`,
  `RpcStateRoot`, `BlockPersistOutcome`.
- Preserve typed errors. Avoid strings, `Box<dyn Any>`, and generic
  "operation failed" messages across layer boundaries.
- Make query cardinality explicit. If an RPC or VM query expects one item,
  expose a typed `single_*` or decoder that fails with domain context.

## Module Organization

Crate roots are maps, not implementation dumping grounds.

- `lib.rs`, `main.rs`, and `mod.rs` should show module structure, public
  facades, and deliberate re-exports.
- Implementation files belong under domain folders such as `fast_sync/`,
  `ledger_source/`, `storage/`, `service/`, `protocol/`, `rpc/`, `network/`,
  `runtime/`, or existing local equivalents.
- Avoid many loose files directly under a crate root. When a root gains more
  than a few sibling implementation files, create a domain directory.
- Re-export only the facade types needed by higher layers.
- Keep `support/` and `test_support/` subordinate to real domains. They must
  not become hidden owners of protocol behavior.
- Tests should mirror production module structure where possible.
- If a reorganization changes paths only, avoid behavior edits in the same
  patch unless the behavior change is required and tested.

Preferred shape:

```text
neo-node/src/node/fast_sync/
|-- mod.rs          # public story and re-exports
|-- workflow.rs     # high-level use case
|-- package.rs      # location, download, extraction
|-- manifest.rs     # parsing and checksum policy
|-- import.rs       # chain.acc import orchestration
`-- report.rs       # named outcomes
```

Avoid:

```text
neo-node/src/node/
|-- fast_sync.rs
|-- fast_sync_package.rs
|-- fast_sync_manifest.rs
|-- fast_sync_import.rs
|-- fast_sync_utils.rs
`-- fast_sync_helpers.rs
```

The first shape gives the concept an address and keeps the parent module
readable. The second leaks implementation detail into the parent module.

## Naming Rules

Use names that teach the domain flow.

Good workflow verbs:

- `open_ledger_source`
- `restore_fast_sync_package`
- `verify_package_manifest`
- `import_chain_acc`
- `persist_block_batch`
- `apply_native_contract_changes`
- `flush_state_root`
- `serve_rpc_methods`

Good result names:

- `FastSyncReport`
- `LedgerOpenOutcome`
- `BlockPersistOutcome`
- `StateRootCommitReport`
- `RpcOnlyRuntimeHandle`

Weak names:

- `process`
- `handle`
- `execute`
- `do_work`
- `manager`
- `helper`
- `runner`
- `context`

Generic names are acceptable only when the surrounding type supplies the domain
meaning. `ApplicationEngine::execute` is understandable; a free function named
`execute()` in a service module is not.

## Extension Traits

Extension traits may create a local fluent vocabulary when inherent methods are
not possible, but they must be narrow and discoverable.

- A trait named `ContractScriptExt` with script-building helpers is acceptable.
- A broad `NodeExt`, `HelperExt`, or `UtilsExt` trait with unrelated RPC,
  storage, and VM behavior is not.
- Re-export extension traits deliberately from a module that reveals the
  vocabulary.
- Do not use extension traits to bypass ownership boundaries between crates.

## Error and Observability Rules

- Error variants and context should name the domain operation that failed.
- Low-level errors should be mapped once at the boundary that understands their
  meaning.
- Library errors should remain typed. Use `CoreError`, crate-local error enums,
  or structs with stable variants/fields where callers need to react.
- `anyhow` is allowed for binaries, CLI orchestration, developer tools, and
  tests. Do not let it become a public library API unless the caller truly
  cannot make decisions from the error.
- Async task errors that cross Tokio task or channel boundaries must satisfy
  the required `Send + Sync + 'static` contracts explicitly.
- Error-path tests should assert the variant or stable message for malformed
  external input, consensus-relevant validation, and storage/RPC boundary
  failures.
- Metrics and tracing should use stable operation names that match workflow
  verbs. If the code says `import_chain_acc`, the metric should not be only
  `process_time`.
- Logs should report decisions and state transitions, not every helper call.
- Consensus-critical validation failures must not be hidden behind generic
  errors.

## Testing Rules

Tests should make intended behavior easier to learn than reading implementation
detail.

- Name tests as behavior sentences, preferably under a module named for the unit
  or workflow being tested.
- Keep one behavior per test. Multiple assertions are acceptable when they prove
  one observable contract.
- Cover error paths before refactoring parsing, consensus validation, storage
  import, RPC decoding, or VM interop.
- Use property tests for codec round trips, state-transition invariants, and
  parser rejection behavior when the input space matters more than examples.
- Fuzz network messages, transactions, witness/script parsing, RPC parameters,
  and persisted format decoders.
- Use deterministic simulation or multi-node integration tests for P2P, sync,
  mempool propagation, consensus timing, reorgs, partition recovery, and
  backpressure.
- Put focused unit tests near the implementation when private details matter.
  Use integration tests for public workflows, CLI modes, and service
  composition.
- Use doc tests for public examples when they are stable and cheap to run.
- For performance claims, record the command, dataset, release profile, and
  before/after result. Sync-speed claims need BPS evidence, not code inspection.

## Repository Hygiene

Only source-controlled artifacts belong in git.

Keep these out of the repository:

- local ledgers and RocksDB directories
- downloaded fast-sync packages
- extracted `chain.acc` files
- checkpoints and replay outputs
- runtime logs and metrics dumps
- ad hoc benchmarking data
- large generated artifacts

Commit fixtures only when they are small, deterministic, named, and documented
for a specific test. Put durable fixtures under an explicit fixture directory;
do not mix them with operator runtime data.

## Required Documentation for New Abstractions

When adding a workflow facade, extension trait, generic abstraction, or
trait-object boundary, document:

- The business operation it represents.
- The layer that owns it.
- Possible side effects: network, disk, state root, mempool, consensus
  validation, shutdown, or none.
- Whether it is a compile-time generic boundary or runtime `dyn Trait`
  boundary, and why.
- The C# parity rule if it exists to match reference-node behavior.
- The focused tests or benchmarks that protect it.

If this explanation is hard to write, the abstraction is probably too wide or at
the wrong layer.

## Review Rejection Triggers

Push back on changes that introduce these shapes without a documented reason:

- Top-level orchestration that requires reading storage, RPC, VM, or wire
  mechanics to understand the business sequence.
- Trait objects passed through multiple layers when production uses one known
  implementation.
- Public generic parameters that exist only for one private helper.
- Fluent chains that hide retries, consensus checks, persistent writes,
  fallback behavior, or shutdown decisions.
- Raw JSON, stack items, byte vectors, or primitive tuples crossing boundaries
  where named Neo types would explain the contract.
- New loose files under a crate root when the feature has or deserves a domain
  folder.
- `utils`, `helpers`, or `misc` modules owning protocol, storage, runtime, or
  RPC behavior.
- Errors, logs, or metrics that use generic names instead of the domain verb.
- Abstractions that add allocation, boxing, missed batching, or dynamic dispatch
  to hot paths without evidence.

## Refactoring Guidance

Apply this style incrementally. Do not rewrite a working subsystem only to make
it fluent.

When improving existing code:

1. Lock behavior with the narrowest existing or new tests.
2. Identify one noisy top-level workflow.
3. Name the intended domain sentence in plain language.
4. Extract repeated mechanics into lower-layer operations with domain names.
5. Keep important branches and side effects visible.
6. Return named outcomes at workflow boundaries.
7. Re-run focused tests, formatting, and any relevant parity or performance
   checks.

The style is successful when the top layer is easier to read, lower layers are
easier to test, and protocol correctness plus sync throughput are preserved.
