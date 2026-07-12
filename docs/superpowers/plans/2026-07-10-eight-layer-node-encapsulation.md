# Eight-Layer Node Encapsulation Plan

**Status:** Complete. Lifecycle staging, provider-neutral core assembly,
service API boundaries, architecture enforcement, documentation, and workspace
verification are implemented. Existing typed sync, startup-import,
live-service, and shutdown operations remain module-owned stages beneath
`RunningNode`; separate nominal wrapper structs are not required unless they
acquire independent callers or state.

**Goal:** Make the daemon read as a short sequence of Neo node operations while
each lower layer owns and hides its protocol, execution, networking, and
storage mechanics.

**Protocol constraint:** The refactor must preserve the already completed Neo
N3 v3.10.1 behavior. No phase changes wire bytes, native-contract semantics,
state-root calculation, or persistence ordering.

**Implementation rule:** Every phase starts with a behavior or architecture
guard, moves one ownership boundary, removes the superseded entry point, and
runs the affected crate tests before continuing.

## Layer Contract

| Layer | Crates | Public vocabulary | Hidden mechanics |
| --- | --- | --- | --- |
| Foundation | `neo-primitives` | domain value types | byte codecs and services |
| Infrastructure | `neo-io`, `neo-error`, `neo-crypto`, `neo-storage`, `neo-config`, `neo-vm`, `neo-serialization`, `neo-manifest` | precise mechanical capabilities | node policy and lifecycle |
| Protocol | `neo-payloads`, `neo-consensus`, `neo-hsm` | blocks, transactions, witnesses, dBFT messages | stores, RPC, process startup |
| Domain services | `neo-runtime`, `neo-execution`, `neo-native-contracts`, `neo-state-service`, `neo-mempool` | import, execute, persist, state, admission contracts | CLI and transport wiring |
| Node services | `neo-blockchain`, `neo-network`, `neo-wallets`, `neo-indexer`, `neo-oracle-service` | long-running service handles and provider APIs | channel loops and lower codecs |
| Composition | `neo-system` | built node, sync workflow, service lifecycle | concrete worker and channel assembly |
| Plugin boundary | `neo-rpc` | operator/client service APIs | core-node construction |
| Application | `neo-node`, `neo-gui` | start, import, serve, stop use cases | protocol, store, and channel mechanics |

Dependencies may point downward, but application code should normally call the
next stable facade instead of reaching through several layers. A justified
tooling path such as `neo-db-probe` may use lower mechanical APIs directly, but
must remain isolated from the daemon workflow.

## Phase 1: Application Lifecycle Boundary (Implemented)

**Files:**

- Add `neo-node/src/node/application/mod.rs`.
- Add `neo-node/src/node/application/command.rs`.
- Add `neo-node/src/node/application/runtime.rs`.
- Reduce `neo-node/src/node/lifecycle/daemon.rs` to the ordered application facade.
- Add `neo-node/src/tests/node/application.rs` and register it from the node
  test module.

**API:**

```rust
NodeCommand::from_cli(NodeCli::parse())?
    .open_runtime()
    .await?
    .run_requested_mode()
    .await
```

`NodeCommand` owns CLI validation. `OpenNodeRuntime` represents either an
operator-requested preflight exit or a fully opened runtime. `NodeRuntime` owns
logging guards, observability, the composed node, startup import state, live
service guards, and graceful shutdown. Only legal lifecycle stages expose the
next operation.

**Regression gates:**

- Conflicting remote-ledger/import modes are rejected before runtime opening.
- Preflight exit does not construct node services.
- The daemon module contains no direct calls to `build_node`,
  `run_startup_imports`, `start_live_services`, or `run_daemon_shutdown`.
- Existing `neo-node` runtime, restart, fast-sync, RPC, and shutdown tests stay
  green.

## Phase 2: Core Composition Ownership (Implemented)

Provider-neutral core assembly moved from
`neo-node/src/node/lifecycle/composition.rs` to `neo-system`:

1. `NodeCoreBuilder<P, S, H>` takes required settings, storage, native provider,
   static commit hooks, persisted height, and optional stop height.
2. It creates the canonical `StoreCache`/snapshot, mempool, header cache,
   ledger context, `NodeSystemContext`, and `BlockchainService`.
3. `NodeCoreLaunch::into_parts` returns a shareable `NodeCore` plus an owned
   `BlockchainTask`; the application supervisor runs only the named task.
4. `NodeCore::into_node(network)` consumes the staged core and guarantees the
   final `Node` uses the same store, provider, mempool, caches, and blockchain
   handle.
5. Concrete generic types (`P`, `S`, `H`) and the closed `RuntimeStore` enum
   keep the core path statically dispatched.
6. Keep CLI config parsing, HSM credentials, RPC, oracle, telemetry, and
   process observability in `neo-node`.

Do not make `neo-system` depend on `neo-rpc`, `neo-oracle-service`, or
`neo-node`. Optional application services receive typed handles from the core
composition result.

## Phase 3: Use-Case Facades (Implemented Without Nominal Wrappers)

`RunningNode` owns the process resources and orders the existing typed stages:

- `StagedSyncPipeline` composes durable header verification with
  `SyncImportPipeline`; `SyncDownloadImportDriver` admits only header-matching
  bodies to canonical `BlockImport`, checkpoints progress, and reports results.
- `run_startup_imports(StartupImportContext)` selects chain.acc or built-in
  fast sync, restores durability, and returns `StartupImportOutcome`.
- `start_live_services` starts P2P, RPC, telemetry, and optional read services
  and returns `LiveServiceGuards`.
- `run_daemon_shutdown` waits for the named trigger, cancels children, flushes
  state, and restores durable mode.

`NodeRuntime` calls only `RunningNode::run_requested_mode`; it does not
destructure task handles, stores, or service bundles. The stage functions stay
independently testable and do not gain wrapper types whose only job would be to
forward the same arguments.

Process cancellation is application policy owned by `RunningNode`.
`neo-system::Node` does not create a second token or expose a competing
`run`/shutdown lifecycle.

Consensus-critical branches and irreversible commits stay as named `match` or
`if` blocks inside the owning lower layer; they are not compressed into fluent
chains.

## Phase 4: Enforced Boundaries (Implemented)

Extend `tests/tests/architecture/layer_boundary_tests.rs` to enforce:

- no upward production dependencies;
- an explicit allow-list for same-layer dependencies;
- `neo-runtime` remains a low-level contract crate;
- `neo-system` has no plugin/application dependencies;
- lower service crates never import `neo-system` or `neo-node`;
- active application startup goes through the lifecycle facade.

The manifest boundary check covers required and build dependencies, including
target-specific tables. It intentionally excludes `dev-dependencies`: test
fixtures may depend upward without adding an edge to the production node
graph.

Restrict public modules after call sites migrate. Prefer named provider traits,
associated types, generics, and closed enums. Production `dyn` is limited to
the standard panic-payload ABI exposed by `catch_unwind`; core service,
storage, VM, native-contract, signer, wallet, and pipeline boundaries are
statically dispatched.

The enforced service API also keeps `neo-blockchain` command-loop internals and
`neo-network` wire/protocol module layouts private. Required capabilities and
wire types are re-exported at crate roots, so callers depend on stable types
rather than source-file organization.

## Phase 5: Verification And Documentation (Completed)

Run, in order:

```text
cargo fmt --all -- --check
cargo test -p neo-node --bin neo-node
cargo test -p neo-system --lib
cargo test -p neo-runtime --lib
cargo test -p neo-tests --test layer_boundary_tests
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets --profile test
```

Update `docs/architecture.md`, `docs/dataflow.md`,
`docs/coding-design-architecture-guidance.md`, and `design.md` only with
implemented status. Any deferred boundary remains explicitly marked as such;
documentation must not describe a planned facade as complete.

All listed commands passed on 2026-07-10. The workspace test run covered unit,
integration, architecture, and doctests; Clippy completed for every workspace
target under the test profile. The separate architecture-document test suite
also passed.

## Completion Criteria

- The daemon entry point uses only application-level node verbs.
- Reusable core composition lives in `neo-system`, not in the binary.
- Sync, startup import, live service, and shutdown are named, module-owned
  stages with explicit typed inputs and outcomes; forwarding-only nominal
  wrappers are not required.
- Cross-layer production and build dependencies are mechanically checked.
- No protocol or state-root parity test changes are needed to make the
  architecture pass.
- Workspace tests and Clippy pass with no ledger or build artifacts committed.
