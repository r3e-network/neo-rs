# neo-rs Crate Boundary Refactor Plan

Goal: every crate owns one functional area with clear, complete, non-overlapping
boundaries. neo-core (89,022 LOC) is the monolith to decompose. Each increment must
end with a green `cargo build --workspace`.

Baseline restored on branch `refactor/restore-green-baseline` (commit f59c13d5).

## Dependency layering (target, downward-only)

```
neo-primitives → neo-crypto / neo-io / neo-config / neo-storage → neo-vm-rs
   → neo-core → neo-p2p? / neo-rpc / neo-consensus / neo-telemetry → neo-node
```

Hard constraints (verified from Cargo.toml):
- neo-rpc depends on neo-core (not the reverse). Orchestration types
  (`NeoSystem`, `services::traits`, `events`) are consumed by neo-core internals
  AND neo-rpc, so they CANNOT move "up" to neo-node.
- neo-storage depends only on neo-primitives — safe to absorb storage code.
- neo-p2p depends only on neo-primitives/crypto/io — must NOT gain a neo-core dep
  (would create a cycle); this gates payload relocation.

## What is ALREADY clean (no work)

- Foundation value types: crypto, io, compression, constants, witness_rule —
  only thin re-export shims remain in neo-core.
- Feature pattern done right: `tokens_tracker`, `application_logs`,
  `state_service`, `oracle_service` use the canonical two-layer split (indexer/
  domain in neo-core; JSON-RPC handlers in neo-rpc). This is the TEMPLATE.

## Work classified by risk

### Tier 0 — Dead-code & shim purge (zero/low risk, green-preserving, do first)

| Item | Location | LOC | Evidence of safety |
|---|---|---|---|
| Orphaned backup manager | `neo-core/src/persistence/backup.rs` | 577 | declared in no `mod.rs`; uncompiled |
| Dead storage-watch subsystem | `neo-core/src/persistence/data_cache/storage_watch.rs` + 5 call sites in `neo_system/persistence.rs` | 171+ | logger has 0 callers; context feeds only the dead logger |
| Phantom test block | `neo-core/src/persistence/storage.rs:88-276` | ~190 | references non-existent `StorageError::{NotFound,Other}` + unimported `CoreError`; makes `cargo build -p neo-core --tests` RED |
| Unused alias | `storage.rs:83` `pub type StorageResult=CoreResult` | 1 | 0 consumers |
| Dead monitoring facade | `neo-core/src/monitoring/` + `tests/monitoring_tests.rs` + `monitoring` feature/deps | 717+378 | only consumer is its own test; duplicates neo-telemetry |
| RpcException shim | `neo-core/src/rpc/` | 8 | re-export of `neo_primitives::RpcException` |
| Trivial foundation shims | `neo-core/src/script_validation.rs`, `extensions/io/mod.rs` | ~96 | pure re-exports of neo_vm / neo_io_crate |
| Intra-neo-storage dup DataCache | `neo-storage/src/cache/` (vs `persistence/data_cache/`) | 1220 | zero external consumers of `neo_storage::cache::*` |

Removable here: ~3,500+ LOC.

### Tier 1 — Low-risk relocations / dedup (green-preserving, small blast radius)

- Network infra with zero neo-core coupling → neo-p2p: rate limiter, `BanList`,
  `PeerReputation`, `validate_peer_endpoint` (`network/p2p/mod.rs:94-543`); add
  `governor` dep to neo-p2p.
- Dedup `MessageCommand`/`MessageFlags`: make neo-core re-export `neo_p2p::*`
  instead of re-instantiating the neo-primitives macro (eliminates two-type problem).
- Drop redundant `network/inventory.rs` (now in neo-primitives).
- Consolidate metrics into neo-telemetry; delete neo-core `telemetry/` (796 LOC)
  + the duplicate `TELEMETRY` static in `neo-node/src/metrics.rs`; consolidate
  `neo-node/src/logging.rs` onto `neo_telemetry::init_node_logging`.
- Codemod-delete foundation shims: `cryptography/mod.rs` (68 refs/53 files),
  inline `neo_io` mod (141 refs/98 files), inline `neo_config` mod (10/9), and the
  `smart_contract/mod.rs:62-67` + `extensions/mod.rs:17` module-path aliases.

### Tier 2 — Epics (need design; separate, sequenced efforts)

1. **VM engine unification (largest).** Local `neo-vm` is still the primary engine
   (`ExecutionEngine`/`JumpTable` ~5.1K LOC); neo-vm-rs interpreter only handles
   trivial single-frame scripts (19 syscalls). Deleting local neo-vm requires
   porting ~15K LOC host code into the `no_std` neo-vm-rs (or completing the
   interpreter) + differential parity tests. Definition of done already exists and
   is RED: `tests/tests/no_local_neo_vm_dependency.rs` (in default-members).
2. **RocksDB provider → neo-storage** (feature-gated): move
   `persistence/providers/rocksdb/` + `StorageConfig`/enums + `write_batch_buffer.rs`;
   mirror `rocksdb` feature in neo-storage; keep neo-core `persistence` as façade.
3. **`ProtocolSettings` consolidation.** Two divergent types: neo-core engine model
   (ECPoint + `HashMap<Hardfork,u32>`, 134 consumers) vs neo-config string model
   (21 consumers, neo-rpc). Never bridged. Unify into neo-config adopting the engine
   model; needs neo-config → neo-crypto/primitives (downward, acyclic).
4. **Transaction/Block/Header data vs verification split.** `*/core.rs` +
   `serialization.rs` are wire-only and movable to a shared layer; all
   `ApplicationEngine` coupling is isolated in `*/verification.rs`. Requires
   inverting `verify()` from inherent method to a neo-core-side trait impl so the
   wire types can drop to neo-p2p/neo-primitives without a dependency cycle.
   `ProtocolMessage` relocation depends on this.

## Sequencing

Execute Tier 0 first (independent, each its own commit, each green). Then Tier 1.
Tier 2 epics are scheduled individually with their own design + parity tests.
The aspirational `no_local_neo_vm_dependency.rs` suite is the executable spec for
the VM epic; do not regress it.
