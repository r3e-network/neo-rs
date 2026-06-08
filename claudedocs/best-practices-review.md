# neo-rs Workspace Best-Practices Refactoring Roadmap

*Synthesis of 23 review reports (per-crate, cross-cutting, clippy baseline) produced
by a parallel agent review. Claims tagged "(verified)" were confirmed against the
working tree. This document drives the refactoring program. Companion to
`crate-boundary-refactor-plan.md`.*

## 1. Executive summary

neo-rs is a substantial, working Neo N3 port. Strengths: `neo-crypto` (constant-time
cmp, zeroize), `neo-consensus` decomposition, storage MVCC snapshot isolation, `neo-io`
var-int handling, `neo-vm` StackItem taxonomy. Workspace uses resolver 2, edition 2024,
a real `workspace.dependencies` table.

But it is mid-refactor and **not production-ready**. Several findings are protocol-
correctness defects, not style. The 7 dominant themes:

1. **Protocol-correctness defects.** MaxStackSize enforcement deliberately disabled
   (`neo-vm/.../execution.rs:~236`, verified — FIXED); wallet helper builds txns with
   `WitnessScope::NONE` instead of `CALLED_BY_ENTRY` (`wallets/helper.rs:762`, verified
   — FIXED); 6 mainnet-block repro tests document state-root divergence vs C#.
   **CORRECTION:** the "single vs double SHA-256 block hash" finding is **REFUTED** —
   see 0.2 below. Neo N3 block/header/tx hashing uses single SHA-256; the current code
   is correct, proven by the now-runnable genesis known-answer test.
2. **Panics reachable from library code.** ~1,611 unwrap/expect outside tests; two on the
   VM hot path (`external_vm.rs:317,353`, verified) → node abort (`panic="abort"` release);
   unbounded `read_var_bytes(usize::MAX)` reachable from on-chain events (OOM DoS).
3. **Duplicate divergent types.** Two `ProtocolSettings` (neo-config vs neo-core, verified);
   two `Block`/`BlockHeader`; two `MessageCommand` (verified); four neo-rpc error types.
4. **No lint enforcement.** No `[workspace.lints]`, `clippy.toml`, `deny.toml`,
   `rust-toolchain.toml` (verified absent); `missing_docs` warned then allowed
   (`neo-core/src/lib.rs:82`, `neo-vm/src/lib.rs:136`, verified); CI suppresses 9 clippy
   categories incl. `assertions_on_constants` (verified).
5. **Cargo hygiene.** Out-of-workspace `neo-vm-rs` path dep pulls dual sha2 (0.10.9 +
   0.11.0, verified); ~30 deps pinned locally; unused async deps in neo-io (verified)/
   neo-p2p/neo-json; workspace `tokio features=["full"]`.
6. **Hollow tests/conformance.** `block_vectors.json` is `[]` (verified); neo-core test
   target red (task #10) → 764 tests unverified; 30+ ignored tests incl. consensus replay.
7. **C#-ism leakage.** `equals(Option<&Self>)`, `get_span`, `hash_code`, `to_byte_array`,
   `try_parse(out)`, `TKey/TValue`; silent `unwrap_or_default`/`unwrap_or(false)` in
   financial/hash paths; neo-primitives tests call a non-existent `equals` → test target broken.

## 2. Per-crate scorecard (1–5; RI idioms, EH errors, ST structure, DT docs/tests, PP parity)

| Crate / Area | RI | EH | ST | DT | PP | Avg | Weakest |
|---|---|---|---|---|---|---|---|
| neo-primitives | 3 | 4 | 3 | 3 | 3 | 3.2 | broken test (DT) |
| neo-config | 2 | 3 | 2 | 2 | 2 | 2.2 | dup ProtocolSettings + HfFaun |
| neo-crypto | 3 | 3 | 3 | 4 | 4 | 3.4 | dual secp256k1 |
| neo-io | 3 | 3 | 3 | 4 | 3 | 3.2 | unused async deps |
| neo-json | 3 | 3 | 3 | 2 | 3 | 2.8 | depth-guard bypass |
| neo-vm | 2 | 2 | 3 | 3 | 3 | 2.6 | MaxStackSize disabled |
| neo-core/smart_contract | 2 | 2 | 2 | 2 | 3 | 2.2 | dual errors, monolith, panics |
| neo-core/network | 3 | 3 | 3 | 4 | 3 | 3.2 | hash-cache &mut self |
| neo-core/ledger-persistence | 3 | 2 | 2 | 4 | 3 | 2.8 | dual Block, hash()→zero |
| neo-core/system-services | 2 | 2 | 3 | 2 | 3 | 2.4 | signer-scope, registry TypeId |
| neo-p2p | 3 | 2 | 3 | 2 | 3 | 2.6 | sync traits + async deps |
| neo-rpc | 3 | 2 | 3 | 2 | 2 | 2.4 | error-code off-by-one, creds |
| neo-consensus | 3 | 3 | 4 | 4 | 3 | 3.4 | f()/m() underflow, single-SHA |
| neo-tee/neo-hsm | 3 | 3 | 3 | 2 | 3 | 2.8 | DER panic, no integ tests |
| neo-telemetry | 2 | 2 | 3 | 2 | 2 | 2.2 | stub MetricsServer, dup metrics |
| neo-node | 3 | 3 | 4 | 3 | 3 | 3.2 | key not zeroized |

Weakest: neo-config, neo-core/smart_contract, neo-telemetry (2.2). Lowest dimension
workspace-wide: docs/tests.

## 3. Refactoring roadmap (sequenced waves; each increment ends green)

### Wave 0 — Correctness P0s (isolated, ship individually)

Each Wave-0 item was independently re-verified against the code + C# Neo by a
verification agent (verdicts: 8 confirmed-bug, 1 nuanced) before any edit.

- [x] 0.1 Re-enable MaxStackSize (neo-vm execution.rs) — DONE (commit 6017bcf2).
      Re-validated: 84 neo-vm unit tests pass with the check on (no over-count).
      Repaired the neo-vm unit-test target (Array::set, hex dev-dep) to validate.
- [x] 0.2 Double-SHA block/tx/header hash — **REFUTED. Do NOT apply.** After fixing
      the neo-core test target (#10), the known-answer test
      `genesis.rs::mainnet_genesis_hash_matches_csharp` was run: it PASSES with the
      current single-`Crypto::sha256` code, and validates against the real N3 mainnet
      genesis hash `0x1f4d…87c15` plus the real genesis next-consensus address
      (`NVg7LjGcUSrgxgjX3zEgqaksfMaiS8Z6e1`). So Neo N3 block/header/tx hashing uses
      SINGLE SHA-256 and the code is correct; switching to double would break consensus
      parity. The review's double-SHA claim over-relied on general Bitcoin-lineage
      lore + the unused `SerializablePayload::hash()` default (which is itself the
      dormant inconsistency — it should be single; low-priority cleanup since unused).
      The state-root divergences have a different root cause (storage-key/native-
      contract layout), NOT the hash function.
      LESSON: this vindicates gating consensus-critical changes on a runnable
      known-answer test rather than agent confidence.
- [x] 0.3 `WitnessScope::CALLED_BY_ENTRY` in wallets/helper.rs — DONE (4beff058).
- [x] 0.4 external_vm.rs unwrap → FAULT — DONE (0bc5d4be).
- [x] 0.5 Cap read_var_bytes in token tracker (3 sites) — DONE (0bc5d4be).
- [x] 0.6 Oracle-id / state-store-init / consensus-time / f()-m() underflow / DER
      panics → Result/guards — DONE (0bc5d4be). (Also fixed a stale hidapi
      double-unwrap so neo-hsm `ledger` compiles.)
- [x] 0.7 RPC credential redaction (manual Debug) — DONE (8b7bfc9e). Zeroizing
      deferred (optional-dep/feature entanglement); Debug redaction is the exposure.
- [x] 0.8 neo-json serde depth guard (MAX_JSON_DEPTH=64) — DONE (0bc5d4be).
- [ ] 0.9 TEE: remove constant fallback key; pass min_counter; drop `simulation`
      default — PENDING (security; verify against enclave counter API).
- [ ] 0.10 RPC error-code off-by-one alignment + compile-time assertion — PENDING
      (macro rewrite to match C# RpcError codes exactly; medium effort).

**Test-target rot — LARGELY RESOLVED (Wave 2.1).** Both neo-vm and neo-core test
targets had pre-existing compile rot (stale StackItem APIs, missing dev-deps,
removed CoreError variants, the Verifiable/VerifiableExt split, relocated
VersionPayload signature, removed `to_array`/`deserialize` APIs). Now green:
- **neo-core: 1302 tests pass** (lib + all integration targets)
- **neo-rpc (--features server): 576 pass**
- **neo-vm: 84**, **neo-storage: 114** pass
- neo-consensus / neo-p2p test targets compile

Fixing neo-core's lib tests revealed (and we fixed) a real functional bug — the
green-baseline iterator-as-Integer shortcut broke native-method iterator results;
they are now `InteropInterface(IteratorInterop)` (C# parity). `ws_events` was
gated behind the `server` feature it requires. Remaining test audit: neo-node /
neo-tee / neo-hsm targets, the `tests` workspace crate, and doctests. With the
core suites green, behavioral protocol changes can now be validated against tests.

### Wave 1 — Hygiene & lint gates
- 1.1 Add `[workspace.lints]`, clippy.toml, deny.toml, rust-toolchain.toml; `lints.workspace = true`.
- 1.2 Remove unused async deps; internal `path`→`workspace = true`; per-crate tokio features.
- 1.3 Drop CI `-A assertions_on_constants`; `cargo clippy --fix`; `#![deny(unused_imports)]`.
- 1.4 Fix let_underscore_future, mutable_key_type (mempool), needless_maybe_sized, HostPtr Send.
- 1.5 Bench/test/fuzz Cargo inheritance; delete `[profile.production]`; bump fuzz pins.
- 1.6 Define `impl_default_via_new!` once in neo-primitives.
- 1.7 Reconcile sha2 / neo-vm-rs (interim pin).

### Wave 2 — Test & conformance foundation (parallel with Wave 1)
- 2.1 Fix neo-core test target (task #10) — unblocks 764 tests; prerequisite for trusting later waves.
- 2.2 Fix neo-primitives `equals` so tests compile.
- 2.3 neo-json missing test module; repoint fuzz to neo-p2p; `cargo fuzz build` in CI.
- 2.4 Re-enable consensus replay test.
- 2.5 Generate C#-derived conformance vectors; wire harness; fail on Inconclusive.
- 2.6 DB-free regression tests from the 6 divergent state-root reproducers.

### Wave 3 — Structural deduplication
- 3.1 Finish neo_config shim (task #9 tail) + delete neo-config::ProtocolSettings; closes HfFaun.
- 3.2 Remove neo_io shim (task #11) — mechanical import migration.
- 3.3 Move application_engine_*.rs into application_engine/ as pub(crate).
- 3.4 Unify VM error variants; dedup memory-pool views; consolidate rpc errors to two.
- 3.5 Move witness types out of neo-io; protocol enums/service traits out of neo-primitives (after 3.2).
- 3.6 Consolidate sha2 / neo-vm-rs per 1.7 decision.

### Wave 4 — Epics
- 4.1 Migrate P2P runtime to neo-p2p (task #12; 16.8k LOC). Gated on 2.1.
- 4.2 Unify MessageCommand (neo-p2p canonical). Depends on 4.1.
- 4.3 Unify Block/BlockHeader (single payload type; remove per-persist clones).
- 4.4 Remove ContractManagement in-memory cache (DataCache-only).
- 4.5 Resolve native-contract protocol-version question (decision + genesis vectors); gates state-root epic.
- 4.6 Consolidate telemetry into neo-telemetry; implement/remove MetricsServer.
- 4.7 Docs push: remove missing_docs overrides; runnable doctests; batch C#-ism renames into one semver bump.

## 4. Best-practice gaps to institutionalize
- `[workspace.lints]`: unsafe_code warn (deny in neo-config/storage/json/consensus), missing_docs warn,
  unused_imports deny, clippy::unwrap_used/expect_used warn (lib), panic_in_result_fn warn.
- `clippy.toml` msrv; `deny.toml` (advisories/bans/licenses — catches dual sha2 & CVE-exposed rustls 0.21);
  `rust-toolchain.toml` pin (edition-2024 + MSRV 1.85).
- CI: fail-fast on test-compile; `cargo deny check`; `cargo fuzz build`; `cargo test --doc`;
  drop `-A assertions_on_constants`; fail if block_vectors.json empty.
- Scope `allow(unsafe_code)` per-module with mandatory `// SAFETY:` (currently missing/incorrect in
  macros.rs:196, bls12381.rs:373, rocksdb/store.rs:375).
- C# conformance vectors from Neo C# v3.9.1; pin the C# tag.

## 4b. Workspace test-suite status (this session)

**COMPLETE — full workspace 3455 passing, 0 failing across 158 test binaries.**
Both aspirational source-inspection suites that gated the VM-boundary work are
now 100% green, and no consensus regression was introduced (genesis KAT, native
contract serialization round-trips, and all NEF/method-token tests intact).

- `tests/tests/layer_boundary_tests.rs` — **11/11 GREEN**.
- `tests/tests/no_local_neo_vm_dependency.rs` (5204 lines) — **121/121 GREEN**.

The earlier "RESOLUTION REQUIRED" note (whether neo-vm-rs is the N4-only VM)
was **resolved by the maintainer**: neo-vm-rs is the *pure, general* NeoVM
implementation intended for all profiles (N3, N4 RISC-V, zkVM). The migration
was then executed carefully and incrementally, green build + consensus guards
per step:

1. **Deleted the local neo-vm crate**; folded the stateful VM *host*
   (ExecutionEngine, JumpTable, host StackItem, serializers, ScriptBuilder,
   storage context) into `neo-core/src/neo_vm`. Pure VM semantics come from
   `neo-vm-rs`. (`4a1defa7`)
2. **Stopped facade-re-exporting** neo-vm-rs symbols from `neo_core::neo_vm`;
   callers import them directly from `neo_vm_rs` (one source of truth). (`23739f43`)
3. **Relocated C# `Neo.SmartContract.*` types** out of the VM host tree to match
   the C# namespace: `BinarySerializer`, `NotifyEventArgs`, `StorageContext`,
   `JsonSerializer` → `smart_contract/`; introduced the `neo_core::vm_runtime`
   seam through which the SC layer imports host VM types. (`5bf7b840`, `a8c8a95d`)
4. **ScriptBuilder → crate root** as a pure byte builder (neo-vm-rs opcode/
   integer metadata, core error types, no local `Script`/`VmError`). (`e48b7990`)
5. **Moved domain types out of the foundation crates** into their correct
   layer: `WitnessRule`/`WitnessCondition` (neo-io → neo-core), `CallFlags`
   (neo-primitives → neo-core/smart_contract), `MethodToken` (neo-io →
   neo-core/smart_contract). neo-io/neo-primitives now carry no
   smart-contract/payload concepts. (`0415e028`, `6bcbfd13`)
6. **Pure persisted state now projects through `neo_vm_rs::StackValue`**
   directly (Transaction, TransactionState, OracleRequest, NeoToken account/
   candidate/committee/governance state, GAS account state) and serializes via
   `BinarySerializer::serialize_stack_value` — **byte-identical** to the prior
   StackItem path (verified by genesis KAT + native-contract round-trips).
   (`4ad3ad77`, `14d983b9`, `e09e0b15`)

The earlier worry that the suite "asserts CallFlags lives in smart_contract/
but layering placed it in neo-primitives" was itself the bug: C# `CallFlags`
*is* `Neo.SmartContract.CallFlags`, so the suite was right and neo-primitives
was the wrong home. Fixed in step 5.

Every other test target — neo-core (lib+integration), neo-rpc (+server),
neo-storage, neo-consensus, neo-p2p, neo-node, neo-tee, neo-json,
neo-primitives, neo-io, doctests, examples, layer_boundary_tests — passes.

## 5. Risks & sequencing
- **Green build ≠ protocol conformance.** Wave 0 must precede any "it works" claim — the node would
  currently fork from mainnet (MaxStackSize, single-SHA, signer scope, 6 state-root divergences).
- **Task #10 (red neo-core tests) gates Waves 3–4** — no large refactor is validatable until they pass.
- Sequencing hazards: #11 before neo-rpc dep slimming; #12 before MessageCommand unify; 3.5 after 3.2;
  state-root epic (4.5) after 2.6.
- **neo-vm-rs decision is load-bearing** (its "Neo N4 RISC-V/zkVM" description raises whether its VM
  semantics even match N3) — resolve with 4.5.
- Effort tags are relative sizing, not calendar estimates, and assume a green neo-core test target.
