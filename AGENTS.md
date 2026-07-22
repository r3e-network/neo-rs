# neo-rs Development Guide for Agents

neo-rs is a production Neo N3 full node in Rust. Correctness is defined by Neo
N3 v3.10.1 protocol behavior, Neo C# parity, and deterministic MainNet replay.
Performance matters only after those properties are preserved.

Read `docs/coding-design-architecture-guidance.md` and `docs/STYLE.md` before a
substantial change. This file is the short operational contract.

## Priorities

Apply these in order:

1. Deterministic Neo N3 v3.10.1 protocol correctness.
2. Exact execution, storage, state-root, serialization, and network parity.
3. Clear ownership and crate boundaries.
4. Measured performance and bounded resource use.
5. Operator usability and documentation.

Never trade replay correctness or state consistency for throughput. Do not
claim compatibility or performance from unit tests alone.

## Workspace Boundaries

The canonical layer model is declared in the root `Cargo.toml` under
`workspace.metadata.architecture` and enforced by architecture tests.

- Foundation: `neo-primitives` owns small domain value types.
- Infrastructure: `neo-config`, `neo-crypto`, `neo-trie`, `neo-io`,
  `neo-error`, `neo-storage`, `neo-static-files`, `neo-state-packs`,
  `neo-checkpoint`, `neo-vm`, `neo-serialization`, and `neo-manifest` own
  mechanical concerns.
- Protocol: `neo-payloads`, `neo-consensus`, and `neo-hsm` own wire payloads,
  dBFT, and consensus signing.
- Domain services: `neo-runtime`, `neo-execution`, `neo-native-contracts`,
  `neo-state-service`, and `neo-mempool` own typed service contracts and state
  transitions.
- Node services: `neo-blockchain`, `neo-network`, `neo-wallets`, `neo-indexer`,
  and `neo-oracle-service` own concrete node capabilities.
- Composition: `neo-system` wires concrete implementations into an embeddable
  node without owning protocol policy.
- RPC boundary: `neo-rpc` owns RPC models, transport, handlers, and RPC plugin
  adapters. Node-level plugin lifecycle and policy are composed in `neo-node`.
- Application: `neo-node` owns CLI, configuration loading, process lifecycle,
  service supervision, and operator-facing behavior.

Dependencies point downward. Higher layers request typed capabilities; they do
not reopen databases, inspect MPT internals, decode VM stacks, or bypass the
canonical block-import path.

## Reth-Derived Subsystem Shape

Use this map when deciding where a new capability belongs. It is the current
composition shape, not a request to copy Ethereum semantics:

- **Types and cryptography.** `neo-primitives` owns protocol-neutral Neo value
  types (`UInt160`, `UInt256`, inventory and witness records). `neo-crypto`
  owns hashes, Merkle construction, curves, keys, and signatures. Payload
  encoding stays in `neo-payloads`/`neo-serialization`; do not add a second
  hash, key, address, or stack-value representation to a service crate.
- **Chain specification.** `neo-config::NeoChainSpec` is immutable and shared
  as `Arc`. `ChainSpecProvider` is the narrow read capability. Hardfork and
  network decisions belong on the spec, while operator limits remain in
  service config such as `TxPoolConfig`.
- **Storage and providers.** `neo-storage` owns `Store`,
  `TransactionalStore`, typed table codecs, `StoreCache`, and durable commit
  fences. `neo-blockchain` and `neo-state-service` own Ledger/State provider
  factories that freeze a height/root and return concrete read views. RPC and
  P2P consume those views and never open a backend or inspect MPT nodes.
- **Execution and engine.** `neo-execution::ApplicationEngine<P, D, B>` is
  composed from a native provider, diagnostic policy, and cache backing. The
  caller chooses concrete generic collaborators; `neo-vm` remains the only VM
  implementation. A plan, specialization, or optimistic artifact is an
  opt-in accelerator around this engine, never a replacement semantic engine.
- **Pool and import engine.** `neo-mempool::MemoryPool<P>` owns admission,
  indexes, priority queues, revalidation, and removal decisions. Typed
  `TransactionOrigin`, provider capabilities, and admission outcomes cross
  the boundary. `neo-runtime::BlockImport`/`ImportQueue` own the generic
  import contract and bounded preflight; `neo-blockchain` owns the canonical
  execution and durable publication implementation.
- **P2P.** `neo-network::PeerRegistry` owns live peers and pending connection
  state. `NetworkHandle` is a capability facade over commands and authoritative
  snapshots. The headers/bodies downloader owns bounded range assignment and
  ordering; broadcast events are notifications, not a second peer database.
- **RPC and application.** `neo-rpc` owns wire models, codecs, client/server
  transport, and plugin method groups. `neo-node` owns daemon lifecycle and
  composes `RpcServices` from immutable node capabilities. RPC handlers must
  not become a second ledger, mempool, or execution implementation.

For a new cross-layer feature, first identify its owner, its smallest
capability trait, and the immutable state it freezes. Add a generic only when
the concrete collaborator is selected by composition or a test needs a second
implementation; do not introduce a trait-object service locator or a generic
`utils` crate merely to share a helper.

## Protocol Authority

- Workspace `neo-vm` is the sole VM semantic authority. Do not introduce
  `neo-vm-rs`, `StackValue`, parallel VM object graphs, or stack-item conversion
  bridges.
- `neo-execution` owns the Neo host/application engine around `neo-vm`; native
  contract behavior belongs in `neo-native-contracts`.
- Preserve byte-for-byte Neo C# behavior for codecs, hashes, witnesses, NEF,
  manifests, native state, storage keys, MPT nodes, and network payloads.
- `neo-network` owns `MessageCommand`, `MessageCommandParseError`, and
  `NetworkError`; shared values such as `InventoryType` remain in
  `neo-primitives` and must not be re-exported through network facades.
- Consensus and execution code must not depend on nondeterministic iteration,
  floating point, wall-clock time, or unordered merge results.
- MainNet replay, hardfork-boundary fixtures, persisted StateRoot equality, and
  reference-node RPC comparison are authoritative integration evidence.
- StateRoot is disabled by default at the CLI and enabled explicitly with
  `--enable-stateroot` or `--stateroot true`. This operator default does not
  reduce the requirement that the enabled path be correct and fast.

## Chain Specification

`NeoChainSpec` is the immutable source of truth for a Neo network. It owns or
identifies network magic, address version, genesis, committee and validator
configuration, hardfork schedule, protocol limits, and seed/bootstrap data.

- Pass a shared `Arc<NeoChainSpec>` from application configuration through
  composition to consumers that need chain rules.
- Put fork activation queries and chain identity on the spec; do not scatter
  magic numbers or reconstruct settings in services.
- Keep runtime/operator settings separate from consensus chain identity.
- Builders may assemble private/custom specs, but a built spec is immutable and
  validated before node startup.
- When migrating chain rules into `NeoChainSpec`, update all callers and delete
  superseded settings types, adapters, aliases, and compatibility branches.
  Compatibility code is justified only by a named external wire, database,
  RPC, or public API contract with tests proving that requirement.

Do not copy Ethereum fork-choice or execution semantics from Reth. Borrow its
typed composition and provider patterns while retaining Neo dBFT, NeoVM,
native-contract, and StateService semantics.

## Rust Architecture

- Top-level functions should read as ordered Neo domain operations. Move codec,
  storage, VM, and transport mechanics to their owning lower layer.
- Prefer concrete types, generics, and associated types for known hot-path
  collaborators. Use `dyn Trait` only for a documented, genuinely open runtime
  extension boundary.
- Use the smallest capability trait. Define it near the domain that consumes
  the capability and keep concrete implementation details in the lower crate.
- Prefer provider/factory pairs for reads: a factory selects and freezes a
  snapshot, height, or root; the provider exposes bounded domain queries
  without leaking the backend.
- Use builders at composition boundaries to make invalid ownership graphs
  unrepresentable. Required capabilities are required fields, not `Option<T>`.
- Prefer closed enums when runtime configuration chooses among implementations
  shipped by this workspace.
- Avoid `async_trait` and boxed futures in protocol, import, VM, MPT, storage,
  and networking hot paths. Use `impl Future + Send` or associated futures.
- Borrow before cloning, avoid intermediate collections, and keep repeated
  cross-layer data in named structs or enums rather than tuples or byte maps.
- Use typed errors in libraries. `anyhow` belongs in binaries, tools, and test
  orchestration. Production panics require a documented impossible invariant.
- Comments explain why and protect invariants. Public crate/module docs state
  `Boundary` and `Contents` as defined in `docs/STYLE.md`.

Do not add an abstraction merely to resemble Reth. Add it when it removes real
coupling, makes an invalid state impossible, enables static composition, or
eliminates measured hot-path cost.

## Concurrency and Performance

- Profile first with a release build and a representative fixed replay window.
  Use stage timings, allocation evidence, flamegraphs, I/O counters, and durable
  write measurements to identify the current bottleneck.
- Use Tokio for asynchronous I/O. Move blocking I/O and CPU-heavy work off
  executor threads with dedicated workers or `spawn_blocking`.
- A network manager or registry is the sole owner of connected-peer and pending
  dial state. Broadcast events are notifications, not a lossless state replica;
  RPC and telemetry read an owner-maintained snapshot or query the manager.
- Never await a sequence of dial, handshake, DNS, or timeout operations inside
  the main network command loop. Track bounded pending work and feed completion
  back to the manager so shutdown, peer commands, and broadcasts stay live.
- Neo P2P framing has one canonical command-to-payload decode and encode path.
  Fuzz helpers, RPC projections, and session dispatch must reuse it rather than
  maintaining a second payload model or command table.
- Parallelize only work with an explicit ordering and conflict contract.
  Optimistic execution must record bounded read/write observations, detect
  conflicts, invalidate dependent results, and commit canonical blocks in
  height order. `neo_execution::optimistic_execution` currently owns isolation,
  dependency capture, and deterministic validation foundations; scheduling and
  publication remain caller-owned gates. Keep production scheduling opt-in
  until block import proves native/range effects and StateRoot parity.
- Signature preverification may overlap tentative execution, but tickets and
  caches are advisory exact-input artifacts. Canonical NeoVM witness validation
  remains authoritative; no block, descendant state, or event becomes canonical
  until every required verification passes. A failure discards the tentative
  suffix and follows the canonical rejection/retry path; never publish first and
  rely on rollback.
- Caches must be bounded and keyed by every semantic input, including hardfork,
  contract version, trigger, call flags, and relevant state identity. Never
  cache stateful execution output as if it were pure.
- Object pools are allowed only when profiling proves allocation pressure and
  reset logic cannot retain state across transactions or engines.
- Empty-block and specialized execution paths require canonical shadow/parity
  tests, hardfork gates, deterministic fallback, and evidence that the fast
  path produces identical artifacts and state.
- Batch storage work through the canonical transactional overlay. A durable
  commit is fallible; do not publish imported state or events before its fence
  succeeds.

Every accepted performance change must report before/after block throughput
from the same data, height window, binary profile, hardware, configuration, and
durability mode. The primary number is StateRoot-enabled blocks per second;
also report transaction-bearing and empty-block throughput where available.
StateRoot-disabled measurements may be supplemental, never a substitute. Do
not claim the 2,000 blocks/s requirement until a reproducible StateRoot-enabled
MainNet campaign demonstrates it.

## Development Workflow

Keep changes focused and verify in proportion to risk. Start with the smallest
affected crate, then widen checks for shared or consensus-critical behavior.

```bash
cargo fmt --check
cargo check -p <crate>
cargo test -p <crate>
cargo clippy -p <crate> --all-targets --all-features -- -D warnings
```

For workspace-wide or cross-layer changes, use the CI-equivalent gates:

```bash
cargo clippy --workspace --all-targets --profile test --locked -- -D warnings
cargo test --workspace --no-run --locked
cargo nextest run --workspace --no-fail-fast --locked
cargo test --workspace --doc --locked
cargo test -p neo-tests --test layer_boundary_tests
git diff --check
```

If the exact architecture test target changes, locate it under
`tests/tests/architecture/` and run the focused target rather than silently
skipping it. Add unit tests for local behavior, integration tests for crate
boundaries, parity fixtures for protocol bytes, and replay evidence for state
transitions. Use fuzz/property tests for parsers, codecs, and proof logic where
the input space is broad.

## Change Discipline

- Inspect the current implementation and callers before designing a replacement.
- Prefer one clean current model over old/new paths, facades, aliases, feature
  switches, and adapters that preserve obsolete internals.
- Remove dead code and stale documentation in the same logical migration.
- Do not mix unrelated refactors with a correctness or performance change.
- Do not edit generated, vendored, runtime data, chain databases, profiles, or
  large reports unless the task explicitly owns them.
- Preserve user changes in a dirty worktree. Never use destructive reset or
  checkout commands to erase work you did not create.
- Use conventional commit subjects: `feat:`, `fix:`, `perf:`, `refactor:`,
  `test:`, `docs:`, or `chore:` with an optional crate scope.
- A performance commit includes the measured result and report path. A rejected
  experiment is reverted cleanly and documented as evidence, not left behind
  disabled or hidden in a side branch.

## Completion Standard

A change is complete when ownership is clear, obsolete paths are removed,
focused tests pass, formatting and Clippy pass for the touched surface, and the
claimed protocol or performance result has reproducible evidence. Compilation
alone is not completion.
