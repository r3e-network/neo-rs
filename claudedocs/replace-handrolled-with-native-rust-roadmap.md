# Replacing neo-rs Hand-Rolled C# Translations with Native-Rust / reth / Polkadot Solutions

> Source: workflow `wmmt7sl51` (2026-05-30), 4 agents (reth deep-dive, polkadot deep-dive,
> neo-rs hand-rolled audit, synthesis). Companion to
> [reth-polkadot-reuse-strategy.md](reth-polkadot-reuse-strategy.md) and
> [neo-core-decomposition-plan.md](neo-core-decomposition-plan.md).
>
> Historical roadmap: the actor migration described below has since landed.
> The temporary network `TaskManagerService`/`TaskManagerHandle` scaffold was
> later deleted after `BlockDownloadCoordinator` became the sole production
> range-sync owner. References to those types document the pre-migration tree,
> not current architecture.

## 1. Headline Answer: Neither reth Nor Polkadot Uses an Actor Framework

The central question — "what should replace the `neo-actors` Akka-style actor system?" — has an
unambiguous answer, identical from both reference codebases:

**Neither reth nor Substrate/Polkadot uses an actor framework.** A repo-wide source search across
reth for `actix` / `ractor` / `kameo` / `xtra` returns zero hits; none appear in its `Cargo.toml`.
Substrate is the same — no Actix, Riker, Bastion, or xtra anywhere in the node. Both are plain
`async`/`await` on a single multi-threaded `tokio` runtime, using the `futures` crate for poll-based
event loops, with all task spawning funneled through one central lifecycle owner.

What they use instead is the **"driver Future + cloneable Handle + typed command channel"** idiom
(identical in both projects, just named differently):

| Concern | reth | Substrate |
|---|---|---|
| Central task lifecycle | `reth-tasks::Runtime` (aliased `TaskExecutor`) + `TaskManager` | `sc-service::TaskManager` + `SpawnHandle` |
| Long-lived subsystem | one `Future` driven as one tokio task (e.g. `NetworkManager`) | one owning async task (e.g. `NetworkWorker`) |
| Outside-world API | cloneable `NetworkHandle` → `mpsc<Command>` | cloneable `Arc<NetworkService>` → `async_channel<Command>` |
| Request/response | `oneshot::Sender` embedded in the command | same |
| Events / fan-out | `broadcast` / `watch` | notification streams / pubsub |
| Fail-fast | "critical" tasks report panics to `TaskManager` | "essential" tasks resolve the node future on failure |
| Graceful shutdown | broadcast `Shutdown` token | terminate-signal + cascading child `TaskManager`s |

**Therefore `neo-actors` should be replaced with: idiomatic tokio tasks + typed `mpsc` command
channels + `oneshot` replies + `broadcast`/`watch` events, coordinated by one small task-lifecycle
owner.** Concretely, the neo-rs target is:

- **Per actor → one `tokio::spawn`'d async task** running a `tokio::select!` loop over
  `mpsc::Receiver<TypedCommandEnum>` plus its own `tokio::time::interval` timers. The message type is
  a **typed enum, not `Box<dyn Any>`** — the single most important change, because it moves dispatch
  from runtime `downcast` to compile-time matching.
- **Each `*Handle` facade → a thin wrapper over `mpsc::Sender<TypedCommandEnum>`**, with `ask`-style
  calls carrying an embedded `oneshot::Sender<Reply>`. These handles **already exist**
  (`BlockchainHandle`, `LocalNodeHandle`, `TaskManagerHandle`), so the public surface barely moves.
- **One lifecycle owner** modeled on `reth-tasks`: a `TaskTracker` + `CancellationToken` (from
  `tokio-util`) for spawn/shutdown, with panic-propagation only where it matters (Blockchain restart).

**Not speculative for neo-rs: the migration target already exists in-tree.**
`TransactionRouterHandle` (`neo-core/src/neo_system/actors.rs`) implements exactly this pattern —
typed envelope over mpsc + a `JoinHandle` — with no actor facade. The de-actorization is "make the
other six subsystems look like `TransactionRouterHandle`," dismantled handle-by-handle.

### How the three named consumers change

- **P2P local-node + task-manager + remote-node** (`LocalNodeActor`, `TaskManagerActor`,
  `RemoteNode`): closest analog to reth's `NetworkManager`. Each becomes a single driver Future owning
  its mutable state (peer table, known-hashes `HashSetCache`, in-flight task map), polled in a
  `select!` loop. Peers talk to it via cloned handles sending a typed `NetworkCommand` enum; events
  fan out over `broadcast`. The `Scheduler`-driven timers collapse into `tokio::time::interval` arms.
  Wide blast radius but **fully behind the existing handles** — external callers see no change.
- **Consensus** (`ConsensusActor` in `neo-node`): best-positioned, because the protocol-critical dBFT
  logic **already lives in the actor-free `neo-consensus` crate**. The node-layer wrapper today spawns
  *two* tasks — one bare tokio task that does nothing but forward `ConsensusEvent` into the actor's
  mailbox as `ConsensusActorMessage::ServiceEvent`. Collapse to **one** `select!` loop calling
  directly into `neo-consensus::ConsensusService`. `DbftConsensusController` becomes a struct owning a
  `JoinHandle` + `CancellationToken`.

`Scheduler` and `EventStream` need no separate replacement — they **die with the actor crate**:
`Scheduler` → `tokio::time::{interval,sleep}`; `EventStream` → `tokio::sync::broadcast`.

## 2. Prioritized Replacement Table

| # | Hand-rolled component | Native replacement | Crate(s) | Consumers / blast-radius | Protocol-path | Effort | Priority |
|---|---|---|---|---|---|---|---|
| 1 | **neo-actors framework** (ActorRef/System/Props/Mailbox/Supervisor/Watch) | tokio task + typed `mpsc` command enum + `oneshot` reply; lifecycle via `TaskTracker`/`JoinSet` + `CancellationToken` | `tokio`, `tokio-util` (in tree) | 6 actors: Blockchain, RemoteNode, LocalNodeActor, TaskManager, StateVerification, Consensus. Wide, but behind typed `*Handle` facades that already exist | No (internal dispatch) | XL | **P0** |
| 2 | **Scheduler / timer service** (Akka `IScheduler` port) | `tokio::time::interval`/`sleep` armed in each task's `select!`; cancel via `CancellationToken`/`AbortHandle` | `tokio`, `tokio-util` | Dies with each actor it serves | No | M | **P0** (folded into #1) |
| 3 | **EventStream pub/sub bus** (Akka `EventStream` port) | `tokio::sync::broadcast` per event type, or direct typed-handle calls | `tokio` | Thin: `neo_system/context.rs`, `core.rs`, blockchain handlers. Dies with #1 | No | M | **P0** (folded into #1) |
| 4 | **EventManager** (string-keyed `dyn Any` bus) | **Delete** — dead test scaffolding | — | None (self + tests) | No | S | **P1** (do first) |
| 5 | **Neo.IO.Caching dead half**: `IoCache`, `LRUCache`, `FIFOCache`, `ECPointCache`, `ECDsaCache` | **Delete** (~700 LOC); blockchain already uses `lru::LruCache` | `lru` (already used) | Zero non-test consumers | No | M | **P1** (do early) |
| 5b | **Neo.IO.Caching live half**: `HashSetCache`, `RelayCache` | Thin typed wrappers | `hashbrown`/`indexmap`, `lru` | `remote_node.rs`, `task_manager.rs`, `neo_system/relay.rs` | No | M | **P2** |
| 6 | **i_event_handlers trait family** + global static `MESSAGE_HANDLERS` | (a) drop `i_event_handlers` alias + `&dyn Any` sender → concrete `&NeoSystemContext`; (b) instance-scope the registry on `NeoSystem`; (c) `broadcast` for non-protocol observers | `tokio` (broadcast) | CommittingHandler: tokens_tracker, application_logs, state_service, oracle, startup. MessageReceived: consensus filter | **Yes** (committing hook on persist path) | L | **P2** |
| 7 | **ServiceRegistry** (C# `IServiceProvider`/TypeId locator) | **Done:** deleted; `NodeServiceHandles<S>` + `RpcServices<S>` named fields and constructor injection | none (std) | `neo-system`, `neo-node`, RPC/startup composition | No | L | **DONE** |
| 8 | **TimeProvider** (global mutable static clock) | Injected `Clock`/`TimeSource` handle; tests use `tokio::time::pause` | `tokio` (test time) | `validation.rs`, `lib.rs`; consensus/block-timestamp reads | **Yes** (time values protocol-visible) | M | **P2** |
| 9 | **Consensus actor dual-loop** (`neo-node`) | Single `select!` loop calling actor-free `neo-consensus::ConsensusService`; remove forwarder task | `tokio`, `tokio-util` | `neo-node` only; self-contained | **Yes** (consensus ordering) | L | **P1→P2** (high value, low blast-radius — good first real actor migration) |

**Recommended execution order:** #4 → #5 (delete dead code, shrink surface) → #9 (consensus:
smallest blast-radius, already-actor-free core, proves the pattern end-to-end) → #1/#2/#3 (network/
blockchain actors, one handle at a time) → #6/#7/#8 (cross-cutting C#-isms).

## 3. Exact First Executable Step Per High-Priority Item

### #4 EventManager — delete (P1, smallest)
`rg -n "EventManager" --type rust` to confirm zero non-test, non-self references. Then delete the
`EventManager`/`EventHandler` block from `neo-core/src/events/mod.rs` and any `pub use`/`pub mod`
re-export, leaving `i_event_handlers` untouched. Build + test. Pure deletion — if it compiles, correct.

### #5 Dead caches — delete (P1)
For each of `IoCache`, `LRUCache`, `FIFOCache`, `ECPointCache`, `ECDsaCache`, confirm no live
consumers (`-g '!**/tests/**' -g '!*test*'`). Delete the files in `neo-io/src/caching/` and prune
`mod.rs` exports **one file at a time, rebuilding between each**. Keep `HashSetCache`/`RelayCache`.

### #9 Consensus dual-loop — collapse (P1→P2, best proof-of-pattern)
First commit (no-op behaviorally): add `CancellationToken` + `JoinHandle` fields to
`DbftConsensusController` and route shutdown through them, *without* removing the actor — proves the
lifecycle wiring. Then extract the forwarder-task body into `drive_consensus(rx, service, cancel)`
calling `neo-consensus::ConsensusService` directly, behind a differential check before the swap.

### #1 Actor framework — first handle (P0, XL)
Pick **`TaskManagerActor`** (smallest mutable state, no protocol path) first. Define typed
`enum TaskManagerCommand { ... oneshot::Sender<_> ... }` mirroring the messages currently `downcast` in
`task_manager.rs:264`. Implement `run_task_manager(mut rx, cancel)` driver Future; make
`TaskManagerHandle` send `TaskManagerCommand` instead of an erased envelope. Keep the old `impl Actor`
in place but unused, spawn the new driver from the same call site. Build + run `p2p_message_tests` /
`task_manager` tests. Only after green, delete the old `Actor` impl for that one actor. Repeat per
actor; `neo-actors` is deleted when the last consumer is migrated.

### #6 i_event_handlers — instance-scope the registry (P2, protocol-path)
First commit (behavior-preserving): replace the process-global
`static MESSAGE_HANDLERS: OnceLock<RwLock<Vec<...>>>` in `remote_node/message_handlers.rs` with a
field on `NeoSystem`. Do **not** change dispatch order or the `CommittingHandler` firing point. The
`&dyn Any` → `&NeoSystemContext` change is a *second*, separate commit.

### #8 TimeProvider — inject without changing values (P2, protocol-path)
Add `clock: Arc<dyn TimeSource>` to readers (validation, consensus), defaulting to the **same**
system-time source. Leave the global static delegating to/from the injected clock so values are
bit-identical. Delete the global only once all readers take the handle. First commit changes *zero*
produced timestamps.

## 4. Protocol-Safety Firewall

**None of these components sit on the byte-level serialization path.** `neo-io`
(`BinaryWriter`/`MemoryReader`/`Serializable`), `neo-vm-rs`, and the dBFT/P2P message *encoding* are
separate, parity-locked crates, **not touched** by any item here.

### Green zone — pure internal-mechanism (ordinary Rust tests suffice)
- **#1 Actor framework** → tokio tasks + typed channels. Dispatch mechanism only. **Safe.**
- **#2 Scheduler**, **#3 EventStream** → tokio timers / broadcast. Internal plumbing. **Safe.**
- **#4 EventManager**, **#5 dead caches** → deletion of unused code (compiler proves it). **Safe.**
- **#5b HashSetCache/RelayCache** → re-back on `hashbrown`/`lru`; same semantics. **Safe.**
- **#7 ServiceRegistry** → typed fields; resolved at compile time instead of TypeId. **Safe.**

### Red zone — protocol-visible behavior (must be byte-preserving + differential-tested vs C# v3.9.1)
- **#6 i_event_handlers (CommittingHandler)** — fires on block-persist path. Firing point and ordering
  relative to persistence must not move. Differential gate: persist a known block range, assert
  identical committing/committed callback order + identical resulting state root vs C#.
- **#8 TimeProvider** — produced timestamps are consensus-visible. Injection is a mechanism swap;
  wall-clock value semantics must be byte-identical. Differential gate: fixed input clock → identical
  timestamp acceptance/rejection + identical block timestamps.
- **#9 Consensus dual-loop collapse** — protocol logic already in actor-free `neo-consensus`; risk is
  purely event ordering + timer cadence of the node wrapper. Differential gate: replay recorded
  consensus session, assert identical message emission order + identical block produced.

**Firewall rule:** red-zone changes are gated on a passing differential test against C# v3.9.1
*before* deleting the legacy path; green-zone changes gated only on the existing Rust suite staying
green. Keep the old path compiled alongside the new one until the gate passes — never delete-then-verify.

## 5. ADD from reth/Polkadot vs REPLACE that neo-rs hand-rolled

### REPLACE (covered above): actor framework→tokio tasks (#1), Scheduler→tokio timers (#2),
EventStream→broadcast (#3), EventManager→delete (#4), cache hierarchy→`lru`/`hashbrown` (#5),
ServiceRegistry→typed fields (#7), TimeProvider→injected clock (#8).

### ADD (genuinely missing capabilities; honest scope)
| Add | reth/Polkadot | Crate | Note |
|---|---|---|---|
| **Centralized task executor + fail-fast/shutdown** | `reth-tasks::TaskManager`; Substrate `TaskManager`+`SpawnHandle` | `tokio-util` (`TaskTracker`, `CancellationToken`) | **Do as part of #1** — natural home for the actors' supervision/restart. Adopt the *pattern* with `tokio-util`, don't depend on `reth-tasks` (carries reth domain assumptions). |
| **Metrics facade** | reth `metrics`+`#[derive(Metrics)]`+Prometheus; Substrate `substrate-prometheus-endpoint` | `metrics` + `metrics-exporter-prometheus` | Ideal moment to instrument channel depth / task liveness during the channel migration. Low risk. |
| **Tracing bootstrap** | reth `reth-tracing`; Substrate `tracing-subscriber`+telemetry | `tracing`+`tracing-subscriber`+`tracing-appender` | Independent; do opportunistically. |
| **Node-builder composition root** | reth `NodeBuilder`; Substrate `sc-service` builder | own code (pattern) | **Do last** — pays off only after #7 (typed service fields). |
| **Trait-segregated storage provider** | reth `ProviderFactory`; Substrate `sp-database` | pattern + `libmdbx`/`rocksdb` | **Separate initiative** — much larger, orthogonal to de-actorization. |
| **jsonrpsee for RPC** | both: `jsonrpsee` | `jsonrpsee` | **Not during actor work** — protocol-visible (request/response), own differential testing. |

**Bottom line:** de-actorization (REPLACE #1–#3, #7) naturally pulls in exactly **one** thing worth
adding now — a centralized `tokio-util`-based task executor with fail-fast/shutdown. Metrics + tracing
are cheap low-risk adds. Node-builder, MDBX storage, jsonrpsee RPC are real gaps but **separate
initiatives** — folding them in would inflate scope and risk protocol-visible surfaces during what
should be a contained internal-mechanism refactor.

---

## 6. Execution Log + Code-Verified Blocker Analysis (2026-05-30)

### Done (green, committed)
- **#4 EventManager removed** (`refactor(core): remove unused EventManager event bus`) — dead C#
  `Neo.Events` port, only consumer was its own self-test. neo-core integration tests green.
- **#5 dead cache ports removed** (`refactor(io): drop unused LRU/ECPoint/ECDsa cache ports`).
  Audit's dead/live split was **imprecise**: `Cache`/`IoCache` + `FIFOCache` are LIVE (transitively
  via `RelayCache` in `relay.rs` and `HashSetCache` in P2P), so only `LRUCache`/`ECPointCache`/
  `ECDsaCache` + orphaned `LruEntries` were actually dead. neo-io caching tests green (28).
- **Channel-based EventStream subscription** (`feat(actors): add channel-based EventStream
  subscription`) — `EventStream::subscribe_channel<T>() -> mpsc::UnboundedReceiver<T>`, additive,
  auto-prunes on receiver drop, ActorRef path unchanged. 3 new neo-actors unit tests green (15 total).
  **This is the universal enabler** for de-actorizing all three EventStream-consuming actors.

### Code-verified blockers on the actor migrations (why they are multi-session, not rushable)
Reading the actual code (not the audit summary) established the real coupling:

- **Consensus (#9) is the LEAST actor-coupled** of the three EventStream consumers: it uses only
  `ctx.self_ref()` + `ctx.schedule_repeatedly()`; NO DeathWatch, NO peer `ActorRef`s, NO `actor_of`,
  NO `sender()`. Its inputs = EventStream(`PersistCompleted`,`RelayResult`) + the `event_task`
  forwarder(`ServiceEvent`) + scheduler(`TimerTick`) + `ManualStart`. With `subscribe_channel` now
  available, the only real restructuring is the **dynamic per-round service receiver**:
  `install_service` creates a fresh `mpsc::channel(256)` each round (and aborts/respawns the
  forwarder). A single-task `select!` must poll a *swappable* `Option<Receiver>` — clean design:
  loop owns a local `service_event_rx`, the state method sets `pending_event_rx`, the loop swaps it
  after each handler (avoids the `&mut self` vs `&mut self.field` select-borrow conflict).
  **BUT consensus is RED-ZONE and has NO runnable behavioral gate in-tree** (neo-node's only test,
  `block_assembly_test.rs`, tests `BlockData` assembly, not the actor). Migrating it verified only by
  build + unit tests would violate the firewall ("differential gate before deleting legacy path").
  A subtle bug = undetectable-in-CI fork on a validator. → needs a consensus round-replay / multi-node
  gate FIRST.
- **TaskManager (#1) is the audit's recommended first handle and HAS a gate**
  (`task_manager_restart_tests.rs` + 6 inline tests), is green-zone (P2P sync; wrong = slow, not
  fork) — but is **deeply graph-entangled**: it is both a **watcher** (`ctx.watch(&peer)` in
  `session_lifecycle.rs`) and **watched** (`TaskManagerHandle::raw_ref()` "for watcher/runtime
  integration"); peers are `ActorRef`s held in `SessionEntry`; and its gate tests construct it via
  `Props::new(TaskManagerActor::default)`. De-actorizing in isolation needs a **DeathWatch edge
  bridge** (peers must explicitly notify on disconnect, replacing automatic `Terminated`) AND would
  require rewriting the Props-based gate tests — which *defeats* the gate. → best done as part of a
  **cluster-wise P2P migration** (LocalNode + RemoteNode + TaskManager together), so the peer
  `ActorRef` edges and DeathWatch become channel events within one migrated cluster.
- **StateVerification** has the same `ctx`-coupling as consensus (schedule + self_ref + EventStream,
  3 event types incl. `ValidatedRootPersisted`), is protocol-relevant (state root), and has no clear
  runnable gate. → red-zone-ish, defer with consensus.

### Recommended next prerequisites (before any red-zone/cluster migration)
1. **Build behavioral gates**: a consensus round-replay harness (recorded view-changes/timeouts →
   assert identical message-emission order + identical block) and/or a 4-node local testnet smoke
   test. This unblocks #9 and StateVerification safely.
2. **Plan the P2P cluster migration as one unit** (LocalNode+RemoteNode+TaskManager): replace peer
   `ActorRef` edges with typed handles + replace DeathWatch with explicit `PeerDisconnected` channel
   events. Then TaskManager's restart tests can be re-expressed against handles without losing
   coverage.
3. Only then delete `neo-actors` (when the last consumer — Blockchain, the biggest red-zone actor —
   is migrated). `Scheduler`/`EventStream` die with it; the channel-sub path is already in place.
