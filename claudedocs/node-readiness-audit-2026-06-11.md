# neo-rs node readiness audit (2026-06-11)

Evidence-based 6-dimension audit of the Rust Neo N3 node (47 workspace packages,
~45 neo-* libs). Each dimension was checked by running commands / reading code,
not by assertion. Fixes landed this pass are marked ✅.

## Headline
The node **builds, runs, connects to live TestNet peers, and serves JSON-RPC**.
Verified: `cargo build -p neo-node --features wip` → 84 MB binary; started with a
TestNet config, persisted genesis (9 natives), bound the P2P listener, dialed
seeds, completed 5 peer handshakes, `getversion` returned `/neo-rs:0.7.2/`.

## 1. Crate organization — mostly-ok
Clean acyclic DAG (0 cycles), L0 (neo-primitives) → L16 (neo-node), single-
responsibility crates. Leftovers from the neo-core dissolution:
- **neo-chain** — fully ORPHANED (0 reverse deps; 11 stateless `validate_*` fns never called). Source of the build warnings. → delete OR wire into neo-blockchain's (currently absent) stateless block pre-checks.
- **neo-services** — 5 traits with no real impls/consumers (only #[cfg(test)] mocks). Pulls 4 crates onto a no-op dep. → remove or finish.
- Duplicate **BlockchainHandle/BlockchainCommand** in neo-runtime AND neo-blockchain; the live node uses neo-blockchain's. neo-runtime's are superseded (only its `BlockchainEvent` is re-used). → delete neo-runtime's, keep BlockchainEvent.
- **neo-block** misnamed (exports ApplicationExecuted/VerifyResult/TransactionState, not Block). → rename or fix the workspace comment.
- 72 source files still mention dissolved `neo-core` in comments (doc debt).

## 2. Completeness & overlap — mostly-ok
**Zero** `unimplemented!`/`todo!`/`not implemented` in non-test code across all 45
crates. Duplicate-named types examined and judged deliberate-layering (KeyBuilder
wrapper, layered NetworkError) or coincidental (Node). Real items:
- **neo-mempool**: C# `CheckConflicts` pooled-conflict eviction + conflict-fee rebate not implemented (memory_pool.rs:271). → port (medium/high: mempool policy parity).
- neo-blockchain dead stub methods (on_new_block→Succeed, transaction_exists_on_chain→false, conflict_exists_on_chain, validate_transaction) never called. → delete or wire (note: InventoryBlock persists without stateless witness/state-root pre-checks).
- Inert no-op handlers (FillMemoryPool/Idle/DrainUnverified) with no producer → wire or document as intentionally-unwired.
- UnhandledExceptionPolicy duplicated (neo-primitives vs neo-rpc), neither matches C# exactly (cosmetic).

## 3. Protocol coverage vs C# v3.9.1/3.10.0 — (agent died mid-run; prior evidence)
The v3.9.1 + v3.10.0 consensus surface was aligned this session (see
[claudedocs/neo-v3100-parity-plan.md]): all 11 natives method-complete, RPC server
handlers registered, P2P message surface (version/verack/ping/getblockbyindex/
block/getheaders/getdata/tx/extensible/inv/mempool), VM opcode+interop pricing,
HF_Gorgon VM gating. Re-run a dedicated protocol-coverage pass to close the audit.

## 4. Runnability — was major-gaps, now substantially fixed
- ✅ **Synced blocks now persist to the durable store** (was the #1 blocker: the
  daemon's snapshot accumulated writes in-memory only, so the on-disk tip stayed
  at genesis and RPC height stuck at 1). Added `SystemContext::commit_to_store()`,
  called after every persisted block; DaemonContext flushes the retained
  StoreCache (shares state with the snapshot) through to the store. Tested.
- ✅ **Shipped mainnet/production TOMLs start** — `[storage] path = ...` is now
  accepted as an alias for `data_dir` (was aborting). Tested.
- ✅ **README/Dockerfile/entrypoint** corrected: build with `--features wip`,
  real CLI flags, JSON-RPC-over-curl (the documented `neo-cli` binary does not exist).
- REMAINING: fast-sync cursor should read the durable tip post-persist to avoid
  re-fetching already-applied blocks (the "inventory block already persisted"
  spam); add an integration test that syncs to height N and asserts a single
  forward pass + a restart resumes from the on-disk tip.

## 5. Best practices — (see overlap findings; no blockers surfaced)
Error handling via thiserror/anyhow + Result; consensus/state code stub-free.
Open items: the dead-stub removal above; consider flipping `wip` to a default
feature for neo-node now that the migration is complete (the stub `main` is a
migration artifact).

## 6. Documentation — was major-gaps, now partially fixed
High doc volume (ARCHITECTURE.md 50KB, 28 docs/, presets, Docker, Makefile) but
operator instructions had diverged from the real CLI. Fixed README/Dockerfile/
entrypoint (above). REMAINING: README architecture diagram + test-coverage block
still reference `neo-cli`/`neo-chain`/`neo-core`; Makefile `check-*` targets and
DEPLOYMENT.md production commands reference nonexistent flags/neo-cli; no systemd
unit ships. Sweep these to match the real crate layout + CLI.

## Prioritized remaining roadmap
1. (high) fast-sync cursor vs durable tip + sync integration test (completes runnability).
2. (high) neo-mempool CheckConflicts eviction (protocol parity).
3. (med) crate cleanup: delete/ wire neo-chain; remove neo-services + dead stub methods; dedupe neo-runtime BlockchainHandle.
4. (med) doc sweep: README diagram/coverage, Makefile, DEPLOYMENT.md, neo-core comments, systemd unit.
5. (low) UnhandledExceptionPolicy dedup; neo-block rename.
