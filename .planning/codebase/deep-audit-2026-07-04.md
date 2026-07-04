# neo-rs Deep Architecture Audit — 2026-07-04

**Scope**: Brownfield deep audit of the 28-crate `neo-rs` workspace, seeking
refactoring opportunities **beyond** the 26 existing ADRs. Three parallel
audits ran: (1) trait design & abstraction leaks, (2) code duplication & DRY,
(3) crate boundaries & dependency graph.

**Headline**: The 26 ADRs describe a *healthy* codebase (9.4/10), but they
also **codify a number of aspirational abstractions that do not actually run
in production**. The biggest bold move is not "add more abstractions" — it is
**excise dead scaffolding, consolidate the real duplication, and only then
add the few abstractions that pay for themselves.**

---

## Theme A — Dead Code & Fake Seams (HIGH ROI, LOW RISK)

The codebase carries a large surface of traits/types that are documented as
"adopted patterns" or "scaffolded bridges" but have **zero production
consumers**. Each one is a maintenance tax and a misleading signpost for
future contributors.

### A1. `neo-static-files` is an orphaned crate (0 prod consumers)
- **Evidence**: workspace-wide `use neo_static_files` returns 1 hit — its own
  test file. The real cold-archive code (`StaticLedgerArchive`,
  `HotColdLedgerProvider`) lives in `neo-blockchain/src/ledger/static_archive.rs`.
- **Action**: Delete the crate + its `[workspace.dependencies]` entry.
- **ADR coverage**: None (ADR-006 cleaned dead *deps*, not dead *crates*).

### A2. `BlockchainEngineAdapter` is never instantiated
- **File**: `neo-blockchain/src/service/engine_adapter.rs:32`
- **Evidence**: `BlockchainEngineAdapter::new` has 0 call sites. ADR-009/010
  describe it as "the EnginePipeline bridge" — it isn't. All block import
  goes through `BlockchainHandle` directly.
- **Action**: Delete the adapter + the `neo-blockchain → neo-engine` dep it
  creates.

### A3. The entire `neo-engine` public state API is dead
- **Files**: `neo-engine/src/pipeline.rs:107` (`Pipeline::new` — 0 callers),
  `state.rs` (`CanonicalChain`, `ChainTip`, `BlockBuffer` — only used by
  neo-engine's own tests).
- **Evidence**: `neo-engine` has exactly 2 external consumers: the dead
  adapter (A2) and `NeoValidateStage` (ADR-026, "extracted but not wired").
  The whole 6-file L3 crate ships nothing used in production.
- **Action**: Either delete `neo-engine` entirely (move `ValidateStage` trait
  + `NeoValidateStage` impl into `neo-blockchain::pipeline`), OR gate the
  whole crate behind `#[cfg(test)]` until staged sync is actually prioritized.

### A4. Eight dead traits across the workspace
| Trait | File | Impls | Note |
|-------|------|-------|------|
| `AsyncSystemContext` | `neo-blockchain/src/service/service_context.rs:158` | 0 | "Async flavour" never used |
| `ApplicationEngineProvider` | `neo-execution/src/runtime/engine_provider.rs:20` | 0 | Also leaks concrete `ApplicationEngine` return type |
| `WalletChangedHandler` | `neo-payloads/src/execution/event_handlers.rs:163` | 0 (test only) | C#-style plugin never wired |
| `SignerProvider` | `neo-payloads/src/execution/event_handlers.rs:178` | 0 (test only) | Same |
| `AccountLike` | `neo-payloads/src/execution/event_handlers.rs:193` | 0 (test only) | Same |
| `MessageReceivedHandler` | `neo-payloads/src/execution/event_handlers.rs:206` | 0 (test only) | Same |
| `MessageLike` | `neo-payloads/src/execution/event_handlers.rs:218` | 0 (test only) | Same |
| `ConsensusMessage` | `neo-consensus/src/messages/mod.rs:38` | 0 | Real wire type is `ConsensusPayload` struct, doesn't impl the trait |
| `BlockLike` | `neo-primitives/src/blockchain/marker_traits.rs:8` | 0 | Generic `validate_block_size<B: BlockLike>` only called by unwired stage |
- **Action**: Delete all 9. Keep `CommittedHandler` / `CommittingHandler`
  (3 real impls each).

### A5. `BlockchainProvider` — 1 impl, 0 `dyn` consumers, 2 stub methods
- **File**: `neo-runtime/src/node/types.rs:100`
- **Evidence**: `get_transaction_by_hash` and `get_state_root` are `Ok(None)`
  TODOs. Zero `Arc<dyn BlockchainProvider>` consumers — `neo-rpc` uses
  `Arc<NodeContext>` directly.
- **Action**: Either delete the trait, OR finish the 2 stubs AND migrate
  `neo-rpc::NodeContext` to consume `Arc<dyn BlockchainProvider>`. The
  current half-state is the worst of both worlds.

### A6. `OracleNodeProvider` is a 0-method marker trait
- **File**: `neo-oracle-service/src/service/mod.rs:73`
- **Evidence**: `pub trait OracleNodeProvider: ConfigProvider + StoreProvider
  + TxAdmission {}` with a blanket impl. Adds zero behavior.
- **Action**: Delete; replace `OracleService::system` with 3 separate fields.

### A7. `MempoolLike` is a test-mock trait masquerading as an architecture seam
- **File**: `neo-blockchain/src/service/service.rs:345`
- **Evidence**: Docstring admits it exists "so the service can be unit-tested
  with a mock mempool". 5 of 6 impls are test mocks.
- **Action**: Delete; hold `Arc<MemoryPool>` directly.

### A8. Dead `Box<dyn ConsensusSigner>` blanket impl
- **File**: `neo-consensus/src/protocol/signer.rs:53`
- **Evidence**: 0 `Box<dyn ConsensusSigner>` call sites; everyone uses `Arc`.
- **Action**: Delete the `Box` impl (keep `Arc`).

---

## Theme B — Crate Consolidation (MEDIUM ROI, MEDIUM RISK)

### B1. Merge `neo-engine` into `neo-runtime`
- Both L3, both service-trait crates. `neo-engine` has 6 files, 1 consumer
  (which is itself dead per A2). Merging collapses the `EngineApi` (runtime)
  vs `EnginePipeline` (engine) two-vocabulary split that ADR-007 only renamed
  but did not structurally unify.

### B2. Split `neo-rpc` (275 files / 45K LOC) into 3-4 crates
- The `client` feature currently pulls in **12 internal crates** including
  the full execution engine, all 11 native contracts, and the wallet layer.
  A JSON-RPC *client SDK* should not compile the node.
- **Target**: `neo-rpc-api` (types, no internal deps) / `neo-rpc-client`
  (HTTP, deps on api + payloads only) / `neo-rpc` (server) + optional
  `neo-rpc-applicationlogs` / `neo-rpc-tokens-tracker` plugin crates.
- This is ADR-001's deferred work — now justified by the 275-file size.

### B3. Feature-gate `neo-native-contracts` per contract
- 28K LOC, 8 consumers, all-or-nothing deps. `neo-wallets` only needs
  metadata; it currently compiles all 11 contracts.
- **Target**: `features = ["ledger", "neo-token", ...]` with `full` default,
  OR split `neo-native-api` (trait + support) from `neo-native-contracts`
  (implementations).

### B4. Flip `neo-hsm` default features to `[]`
- `default = ["pkcs11"]` pulls PKCS#11 FFI by default. A signing-backend
  crate should not have FFI on by default (compare `neo-tee` which defaults
  to simulation).

---

## Theme C — Native Contract Support Layer (HIGH ROI, MEDIUM RISK)

`neo-native-contracts` has ~265 lines of copy-pasted boilerplate across its
11 contracts. The `support/` module already exists (ADR-025 consolidated
keys there). Extend it:

### C1. `support/codec.rs` — StackValue encode/decode helpers
- `decode_stack_value(bytes, label)` — replaces 14 copies of
  `BinarySerializer::deserialize_stack_value_with_limits` + error mapping
  (~80 lines saved).
- `encode_storage_struct(&T, label)` — replaces 12 copies of
  `serialize_stack_value_default` + `to_stack_value` (~24 lines saved).
- `StructDecoder` helper — replaces 8 `from_stack_value` impls with their
  repeated `StackValue::Struct(_, items)` destructure + `items.get(i)` +
  per-field error wrapping (~70 lines saved).

### C2. `support/engine.rs` — `require_persisting_block`
- Replaces 4 copies of `engine.persisting_block().ok_or_else(...)` prelude
  (~10 lines saved).

### C3. `support/settings.rs` — hardfork-gated i64 setting readers
- `read_hardfork_gated_u32_setting` — replaces 3 `get_max_X_snapshot`
  functions (~55 lines saved).
- Promote Policy's `read_optional_i64_setting_key` /
  `read_required_i64_setting_key` — replaces ~12 BigInt→i64 sites (~25 lines
  saved).

**Net**: ~175 lines removed from a 28K-LOC crate, with a clear pattern for
new contracts to follow.

---

## Theme D — Cross-Crate Helpers (MEDIUM ROI, LOW RISK)

### D1. Promote `invocation_script_from_signature` to `neo-vm::ScriptBuilder`
- 5 copies across `neo-consensus`, `neo-node` (×2), with one hand-rolling
  bytes and one using `ScriptBuilder::emit_push`. Promote to
  `ScriptBuilder::invocation_from_signature(sig)` + inverse
  `signature_from_invocation(script)`. (~28 lines saved + removes
  inconsistency.)

### D2. `now_millis()` in `neo-primitives::time`
- 4 copies (`neo-consensus`, `neo-node` ×2, `neo-rpc`) with 3 different
  names. (~15 lines saved.)

### D3. `elapsed_us` / `elapsed_millis` in `neo-runtime::time`
- 5 sites using `as_micros() as u64` (one with safe clamp, four without).
  Consolidates and removes silent u128→u64 truncation.

### D4. Extend `impl_error_from!` macro for struct-variant CoreError
- 14 `From<DomainError> for CoreError` impls are mechanical
  `Variant { message: err.to_string() }` bodies. Extend the macro to cover
  this form; each owning crate becomes a one-liner. (~55 lines saved.)

---

## Theme E — Test Fixture Consolidation (MEDIUM ROI, LOW RISK)

### E1. Create dev-only `neo-test-fixtures` crate
- `make_transaction` / `make_ledger_block` / `store_block` duplicated across
  `neo-rpc` (×2, ~120 byte-identical lines), `neo-blockchain`, `neo-mempool`.
- Pattern already proven in `neo-native-contracts/src/tests/test_support.rs`.
- **Net**: ~150 lines deduplicated; gives every crate test-isolated fixtures.

---

## Theme F — Async/Blocking Correctness (HIGH RISK, HIGH VALUE)

### F1. `ConsensusSigner::sign` is sync but production signers block on network
- **File**: `neo-consensus/src/protocol/signer.rs:45`
- **Evidence**: `NitroEnclaveSigner`, `Pkcs11Signer`, `AzureKeyVaultSigner`,
  `GcpKmsSigner` all make network/HSM round-trips. The Nitro signer
  docstring admits it spawns its own runtime and blocks the consensus task
  to match the sync seam.
- **Action**: Change `sign` to `async fn` (the crate already depends on
  tokio). Or split `SyncConsensusSigner` + `AsyncConsensusSigner` if
  software signers must stay sync. **This is a correctness/performance
  issue, not just cleanup.**

### F2. `await_wallet_future` hides runtime spawning
- **File**: `neo-rpc/src/server/rpc_server_wallet/mod.rs:264`
- **Evidence**: Takes `Pin<Box<dyn Future>>`, spawns a fresh
  `CurrentThread` runtime per call when no handle is present, can deadlock
  if the wallet future awaits on the parent runtime.
- **Action**: Make it `async fn` with `impl Future`; push the
  `block_in_place`/`spawn` decision up to the RPC handler layer.

---

## Theme G — Architecture Honesty (LOW ROI, LOW RISK)

### G1. Rename `ConsensusService` trait → `ConsensusApi`
- **File**: `neo-runtime/src/service/services.rs:139` (trait) vs
  `neo-consensus/src/service/core.rs:12` (struct).
- Exact analogue of ADR-007's `NeoEngine → EngineApi` fix. The trait has 0
  impls; the struct is the real dBFT state machine.

### G2. Document or delete the aspirational runtime service-trait seam
- `neo-consensus` (L2) sits **below** `neo-runtime` (L3), so it cannot
  implement `ConsensusService`/`EngineApi`/`BlockExecutor` without an
  upward dependency. The traits therefore have 0 implementations.
- **Options**: (a) delete them until a concrete impl is built (honest), or
  (b) move trait definitions to a new L2 `neo-runtime-api` crate so
  `neo-consensus` can implement them (mirrors reth's `reth-interfaces`
  being low-level).

### G3. Extract `Nep17MetadataReader` trait for `neo-wallets`
- `neo-wallets` (L4) pulls in the entire `neo-execution` engine solely for
  `AssetDescriptor` (reads NEP-17 `symbol`/`decimals`). Extract a trait in
  `neo-runtime`; `neo-wallets` depends on the trait, the impl lives higher.

### G4. Deduplicate `StoreProvider`/`ConfigProvider` impls on Node vs NodeContext
- Identical bodies in `neo-system/.../node.rs:427` and
  `neo-rpc/.../node_context.rs:189`. Extract a `StoreConfigBundle` struct
  that implements both traits once; both holders forward to it.

---

## Summary — ROI/Risk Matrix

| Theme | Findings | Lines Saved | Risk | ROI |
|-------|----------|-------------|------|-----|
| A. Dead code & fake seams | 8 | ~400 LOC + 1 crate | Low | **High** |
| B. Crate consolidation | 4 | 1 merge + 1 split | Medium | Medium |
| C. Native contract support layer | 3 | ~175 LOC | Medium | **High** |
| D. Cross-crate helpers | 4 | ~100 LOC | Low | Medium |
| E. Test fixtures | 1 | ~150 LOC | Low | Medium |
| F. Async/blocking correctness | 2 | 0 (restructure) | **High** | **High** |
| G. Architecture honesty | 4 | ~40 LOC | Low | Low |
| **Total** | **26** | **~865 LOC + 2 crate structural changes** | | |

---

## What the 26 Existing ADRs Miss

The existing ADRs are excellent at **documenting decisions that were made**.
They are weaker at **acknowledging decisions that did not pan out**:

1. **ADR-002** claims `neo-engine` is integrated via `BlockchainEngineAdapter`
   — the adapter is never instantiated (A2).
2. **ADR-004** lists `BlockchainProvider` as an "adopted pattern" — it has 0
   `dyn` consumers and 2 stub methods (A5).
3. **ADR-009/010** describe the pipeline overlap as "scaffolded" — the entire
   `neo-engine` public state API is dead (A3).
4. **ADR-023** documents `NodeComponents` type-state as "scaffolded, not
   functional" — but does not flag the 8 *other* dead traits (A4).

The bold recommendation is: **before adding any new abstractions, clean up
the ones that did not pay off.** A new ADR-027 should cover the dead-trait
excision; ADR-028 the native-contract support layer; ADR-029 the
async-signer correctness fix.
