# neo-core Decomposition Blueprint (dissolve the monolith into layered crates)

_Generated 2026-05-30 by the neo-core-decomposition workflow (4 agents). Execute on top of the verified-green Phase 3 parity. Code MOVES only — preserve 100% protocol behavior._

I have everything I need. The key correction over the input analyses: the chain-types target crate is the **existing `neo-p2p`** (which already holds `payloads/`, `verify_result.rs`, `traits.rs`, canonical message types) — not a new "neo-ledger-types". neo-core's `payloads/` re-exports from neo_p2p but also has 14 heavyweight local files (Block/Transaction/etc.) still in neo-core. This is the central thing to resolve.

Here is the final blueprint.

---

# FINAL DECOMPOSITION PLAN: Dissolving neo-core

**Verdict on neo-core:** It does **not** disappear — it **shrinks to a thin facade crate** (`pub use` re-exports only, ~300 LOC of `lib.rs`) kept for one release cycle to avoid a big-bang path rewrite across `tests/`, `neo-node`, `neo-rpc`, `neo-consensus`. The facade is deleted in a final cleanup step once all external consumers repoint to the leaf crates. **Why keep it temporarily:** lib.rs currently re-exports ~40 symbols (`Block`, `Witness`, `ProtocolSettings`, `NeoSystem`, `ScriptBuilder`, etc.) consumed pervasively; a transitional facade lets every extraction step end green without touching downstream crates in the same commit.

**Key correction to the area analyses:** the "neo-ledger-types"/"neo-payloads" target is the **already-existing `neo-p2p`** crate. It already owns `payloads/` (control payloads), `verify_result.rs`, `traits.rs`, and the canonical `MessageCommand`/`MessageFlags`. neo-core's `network/p2p/payloads/mod.rs` already `pub use neo_p2p::payloads::*` for the 10 control payloads but still keeps **14 local heavyweight files** (Block, Transaction, Header, Signer, Witness, attributes, ExtensiblePayload, conditions…). The plan **moves those 14 into neo-p2p**, not into a brand-new crate.

---

## 1. TARGET CRATE DAG (layered, acyclic)

Layers are strictly downward-pointing. A crate may only depend on crates in lower or equal-but-earlier layers.

### Layer 0 — Foundation (exist today, unchanged)
| Crate | Responsibility | Direct deps |
|---|---|---|
| `neo-io` | Binary (de)serialization primitives, var-int, LZ4 compression | — (+ external lz4) |
| `neo-primitives` | UInt160/256, ECPoint scalars, `Verifiable`, `VerificationContext`/`VerificationSnapshot` traits, `Hardfork`, **`CallFlags`** (relocated here) | neo-io |
| `neo-json` | JSON value model | neo-io |
| `neo-crypto` | ECC, hashing, BLS12-381, signatures | neo-primitives, neo-io |
| `neo-vm-rs` | Pure stackless VM (opcodes, `StackValue`, `ExecutionEngineLimits`) | neo-io |

### Layer 1 — Config & Storage
| Crate | Responsibility | Direct deps |
|---|---|---|
| `neo-config` | **Canonical `ProtocolSettings`** (neo-core's typed version, merged in), network magic, hardfork heights | neo-crypto, neo-primitives |
| `neo-storage` | `StorageItem/StorageKey/DataCache/StoreCache/Snapshot/TrackState/SeekDirection`, `CacheProvider` trait — **leaf, no rocksdb, no smart_contract** | neo-io, neo-primitives |
| `neo-storage-rocksdb` | RocksDB backend implementing neo-storage's provider trait (keeps `rocksdb` dep off neo-storage consumers) | neo-storage, neo-io, rocksdb |

### Layer 2 — VM Host
| Crate | Responsibility | Direct deps |
|---|---|---|
| `neo-vm` *(promoted from `neo-core/src/neo_vm`)* | Stateful VM host over neo-vm-rs: execution loop, ref-counted `StackItem`, `Interoperable` trait, interop registry, gas hooks, `BinarySerializer`/`JsonSerializer` (StackItem codecs) | neo-vm-rs, neo-io, neo-primitives |

### Layer 3 — Chain Types (the existing neo-p2p, grown)
| Crate | Responsibility | Direct deps |
|---|---|---|
| `neo-p2p` *(existing, grown)* | **All verifiable chain types + their wire codecs**: Block, Header, Transaction, Signer, Witness, attributes, ExtensiblePayload, conditions, conflicts, oracle_response, inventory, merkle/headers payloads, `WitnessRule`, `witness.rs`; plus `HeaderCache`, `TransactionVerificationContext`, `VerifyResult`, `LedgerContext` (pure data). Control payloads + MessageCommand/Flags already here. Verifies via `VerificationContext`/`VerificationSnapshot` traits — **no edge into VM-engine or native** | neo-primitives, neo-io, neo-config, neo-crypto, neo-storage, neo-vm-rs |

### Layer 4 — Smart Contract Engine & Natives
| Crate | Responsibility | Direct deps |
|---|---|---|
| `neo-native-traits` *(tiny seam)* | `NativeContract` trait + `NativeRegistry` + `EngineHost`/`InteropContext` trait the engine implements | neo-vm, neo-primitives |
| `neo-smart-contract` | `ApplicationEngine` + interop syscalls + manifest/ABI + NEF + `Contract`/`ContractParameter` model + `StorageItemExt`. Implements `VerificationContext`. | neo-vm, neo-p2p, neo-storage, neo-config, neo-crypto, neo-io, neo-primitives, neo-native-traits |
| `neo-native-contracts` | The 11 protocol contracts (ContractManagement, NeoToken, GasToken, Policy, Ledger, Oracle, RoleManagement, CryptoLib, StdLib, Notary, Treasury) | neo-smart-contract, neo-vm, neo-p2p, neo-storage, neo-config, neo-crypto, neo-native-traits |

### Layer 5 — Wallets & Builders
| Crate | Responsibility | Direct deps |
|---|---|---|
| `neo-actors` | Generic tokio actor runtime — **zero neo coupling (verified)** | tokio, tokio-util, async-trait, dashmap, parking_lot, uuid |
| `neo-wallets` | KeyPair, NEP-6, signing-context | neo-primitives, neo-crypto, neo-p2p, neo-smart-contract (or via `VerificationScript` seam), neo-io |
| `neo-tx-builder` | Fluent Transaction/Witness/Signer builders | neo-p2p, neo-primitives |

### Layer 6 — Ledger & Network
| Crate | Responsibility | Direct deps |
|---|---|---|
| `neo-node-traits` *(seam)* | `SystemContext` + `LedgerService/StateStoreService/MempoolService/PeerManagerService/RpcService` + event-handler traits | neo-p2p, neo-storage, neo-config, neo-primitives |
| `neo-ledger` | Blockchain actor + mempool + genesis | neo-p2p, neo-smart-contract, neo-native-contracts, neo-storage, neo-config, neo-actors, neo-node-traits |
| `neo-network` *(transport split out of neo-core)* | P2P transport: local_node, remote_node, task_manager, framing, peer lifecycle | neo-p2p, neo-config, neo-io, neo-actors, neo-node-traits |

### Layer 7 — Services / Plugins / Consensus / RPC
| Crate | Responsibility | Direct deps |
|---|---|---|
| `neo-consensus` *(exists)* | dBFT | neo-p2p, neo-ledger, neo-smart-contract, neo-node-traits |
| `neo-rpc` *(exists)* | JSON-RPC server | neo-p2p, neo-ledger, neo-smart-contract, neo-node-traits |
| `neo-oracle-service` | OracleService plugin (leaf consumer) | neo-node-traits, neo-smart-contract, neo-native-contracts, neo-p2p, neo-wallets, neo-storage, neo-crypto |
| `neo-state-service` | StateService / MPT roots | neo-node-traits, neo-smart-contract, neo-storage, neo-ledger, neo-p2p, neo-wallets |
| `neo-tokens-tracker` | NEP-11/17 indexer | neo-node-traits, neo-smart-contract, neo-storage, neo-ledger, neo-vm-rs |
| `neo-application-logs` | Execution-log persistence | neo-node-traits, neo-smart-contract, neo-storage, neo-ledger, neo-rpc |
| `neo-telemetry` *(exists; absorb neo-core dup)* | Metrics/tracing | dashmap, parking_lot, tracing |

### Layer 8 — System & Node
| Crate | Responsibility | Direct deps |
|---|---|---|
| `neo-system` *(neo_system/ extracted)* | Assembly: wires Blockchain + LocalNode + TaskManager + services; **impls `neo-node-traits::SystemContext`** | neo-ledger, neo-network, neo-smart-contract, neo-native-contracts, neo-node-traits, neo-config, neo-storage |
| `neo-core` *(thin facade, then deleted)* | Transitional `pub use` re-exports for back-compat | re-exports only |
| `neo-node` *(exists)* | Binary entrypoint | neo-system, neo-consensus, neo-rpc, plugins |

---

## 2. TRAIT SEAMS (what keeps the graph acyclic)

| # | Seam (trait) | Lives in | Breaks concrete coupling | reth / polkadot analogue |
|---|---|---|---|---|
| **S1** | **`CallFlags`** (move, not a trait — relocate the bitflags type) | `neo-primitives` | neo_vm→smart_contract back-edge (2 sites: `interop_service.rs:10`, `execution_engine/mod.rs:72`). Engine references `neo_primitives::CallFlags`. **Must be byte/enum-identical** (consensus-observable). | Shared flag/scalar enums live in `reth-primitives` / `sp-core`, never imported upward. |
| **S2** | **`Interoperable`** (already in `neo_vm/interoperable.rs`) + move `BinarySerializer` into `neo-vm` | `neo-vm` | persistence→smart_contract cycle (`storage_item.rs:9-10`). Point persistence/`StorageItemExt` at neo-vm. | `reth-db` codec vs `reth-provider` typed decode. |
| **S3** | **`VerificationContext`** (`verify_witness(&UInt160,&dyn Witness)`) — **already drafted, currently bypassed** | `neo-primitives` | payloads→`ApplicationEngine::new` (the central cycle: `transaction/verification.rs:312`, `header/verification.rs:164`). Payloads take `&dyn VerificationContext`; `neo-smart-contract::ApplicationEngine` impls it. **Extend the trait** to carry snapshot/settings/gas — current draft signature is too thin. | reth: `reth-primitives` tx types stay execution-free; revm `Host` trait implements execution behind the boundary. |
| **S4** | **`VerificationSnapshot` / `HeightProvider`** (`current_index()`) | `neo-primitives` | payloads→`native::LedgerContract::new().current_index()` (7 sites). Payloads read height from a trait. | substrate `sp-runtime` Header/Block traits; reth `StateProvider`. |
| **S5** | **MOVE (not trait): chain data structs down** — `HeaderCache`, `TransactionVerificationContext`, `VerifyResult`, `LedgerContext` | `neo-p2p` | payloads↔ledger bidirectional edge. After move: `neo-ledger → neo-p2p` one-way. | substrate splits `sp-blockchain`/`sp-runtime` types from `sc-client` services. |
| **S6** | **`NativeContract` trait + `EngineHost`** | `neo-native-traits` | ApplicationEngine↔native bidirectional (207 `&mut ApplicationEngine` sites). Engine holds `Vec<Arc<dyn NativeContract>>` injected at construction (dependency inversion); neo-smart-contract does **not** import neo-native-contracts. | revm `Inspector`/`Host`; substrate pallet `Config` trait (runtime wires concrete pallets). |
| **S7** | **`SystemContext`** (~12 methods: store_cache, settings, ledger, header_cache, memory_pool, state_store, record_extensible, broadcast_plugin_event, event_stream, is_fast_sync_mode, self_ref) — **already exists in `services/traits.rs`** | `neo-node-traits` | neo_system↔ledger (`blockchain/mod.rs:57`) and neo_system↔network (`task_manager.rs:31` + handle/state). Actors hold `Arc<dyn SystemContext>`; `NeoSystemContext` impls it. **Audit return types** — must not leak concrete `Arc<NeoSystemContext>` upward. | reth `FullNodeComponents`/`NodeTypes`. |
| **S8** | **Event-handler traits** (Committed/Committing/MessageReceived/WalletChanged) — replace `&dyn Any` with `&dyn SystemContext` | `neo-node-traits` | plugin/state_service coupling to concrete system | substrate `OnFinalize`/hooks defined in traits crate, dispatched by orchestrator. |
| **S9** | **`VerificationScript`/`ContractScriptProvider`** (script bytes + hash) — *optional* | `neo-primitives` | wallets→`smart_contract::Contract` (4 sites). Only needed if wallets must sit *below* smart-contract. **Decision: accept the forward edge `neo-wallets → neo-smart-contract`** (acyclic already) and skip S9 unless a plugin forces wallets below the engine. | reth `reth-primitives` shared types. |

---

## 3. MIGRATION SEQUENCE (each step ends GREEN)

Two phases: **(A) additive seams in-place** (no crate moves, lowest risk, proves the inversions compile), then **(B) physical extraction leaves-first**.

### Phase A — Land seams additively inside neo-core (no new crates yet)

**A1. Relocate `CallFlags` → neo-primitives** *(seam S1)*
Move `call_flags.rs`; re-export from `smart_contract::CallFlags` for back-compat; repoint neo_vm's 2 imports. Risk: **consensus-observable** — add a parity test asserting flag values `{NONE=0,READ_STATES=1,WRITE_STATES=2,ALLOW_CALL=4,ALLOW_NOTIFY=8,...}` unchanged. Effort: **S** (~0.5d).

**A2. Repoint persistence cycle → neo-vm/Interoperable** *(seam S2)*
`storage_item.rs` already imports `Interoperable` from `interoperable` — confirm it points at the VM-host module path, not `smart_contract::`. Effort: **S**.

**A3. Canonicalize `ProtocolSettings`** *(prerequisite for many crates)*
Make neo-core's typed `ProtocolSettings` (with `standby_committee: Vec<ECPoint>`, `Hardfork` keys, `validators_count`) the canonical one, moved into `neo-config`. **Delete divergent `neo-config/src/protocol.rs`** (0 consumers) **only after** byte-for-byte verification against C# Neo v3.9.1. Repoint 59 in-tree consumers `crate::protocol_settings → neo_config`. Risk: **HIGHEST in this phase** — committee/validators/block-time/hardfork heights are consensus/wire. Effort: **M** (1–2d).

**A4. Wire `VerificationContext` + `VerificationSnapshot`** *(seams S3, S4)*
Extend the already-drafted `neo_primitives::verification` trait to carry snapshot+settings+gas. Make `ApplicationEngine` impl it. Replace the 2 inline `ApplicationEngine::new` calls and 7 `LedgerContract::new().current_index()` calls in payloads with trait calls. Risk: **HIGH** — sits on `Transaction.VerifyWitnesses` ordering, Oracle empty-witness, `MAX_VERIFICATION_GAS`. **Move code 1:1, do not rewrite; gate on tx/header/block verification tests + `p2p_message_tests`.** Effort: **L** (3–4d).

**A5. `SystemContext` inversion in-place** *(seams S7, S8)*
Switch `blockchain/mod.rs` + `task_manager` from `Arc<NeoSystemContext>` to `Arc<dyn SystemContext>` (the trait already exists in `services/traits.rs`). Audit every method return type for upward concrete leak. Replace `&dyn Any` in event handlers with `&dyn SystemContext`. Risk: **M**. Effort: **M** (2d).

**A6. `NativeContract`/`EngineHost` seam** *(seam S6 — highest effort)*
Introduce the trait; engine holds injected `Vec<Arc<dyn NativeContract>>`. **Staged:** keep ApplicationEngine + native co-located in neo-core for now (intra-crate bidirectional coupling is legal). This seam only needs to *exist* before B7 splits native out. Risk: **HIGHEST overall** (207 sites, consensus-critical). Effort: **L–XL** (4–6d). **Do NOT split native before this lands.**

> **Gate:** after A1–A6, the full workspace builds green with zero crate-graph cycles *logically broken* even though everything is still in neo-core.

### Phase B — Physical extraction, leaves first

| Step | Extract | Trait used | Risk | Effort |
|---|---|---|---|---|
| **B1** | `neo-actors` (zero coupling, verified) | — | Low | S |
| **B2** | Absorb `telemetry` → existing `neo-telemetry`; delete dup | — | Low | S |
| **B3** | `neo-storage-rocksdb` (move rocksdb provider out of persistence; collapse persistence shim) | S2 | Low-M | M |
| **B4** | `neo-vm` (promote `neo_vm/` + StackItem serializers); fix `vm_runtime`/`script_builder` lib aliases | S1, S2 | M | M |
| **B5** | Grow `neo-p2p`: move 14 chain-type files + witness + witness_rule + HeaderCache/TxVerCtx/VerifyResult/LedgerContext; **collapse duplicate `ledger/block.rs` into `payloads/block.rs`** (diff `serialize()` byte-for-byte first) | S3, S4, S5 | **M-H** (dup-struct drift) | L |
| **B6** | `neo-native-traits` then `neo-smart-contract` (engine framework, no native bodies) | S3, S6 | M | L |
| **B7** | `neo-native-contracts` (the 11 contracts) | S6 | M | M |
| **B8** | `neo-wallets` + `neo-tx-builder` | S9-optional | Low-M | M |
| **B9** | `neo-node-traits` (promote `services/traits.rs` + event handlers) | S7, S8 | Low | S |
| **B10** | `neo-ledger` (actor + mempool + genesis) | S5, S7 | M | M |
| **B11** | `neo-network` (transport) | S7 | M | M |
| **B12** | Plugins in parallel: `neo-oracle-service`, `neo-state-service`, `neo-tokens-tracker`, `neo-application-logs` (all leaf consumers, zero reverse edges) | S7, S8 | Low-M | M each, parallelizable |
| **B13** | `neo-system` (neo_system/ → top crate, impls SystemContext) | S7 | M | M |
| **B14** | Shrink `neo-core` to facade; repoint `neo-node`/`tests`/`neo-rpc`/`neo-consensus`; **delete facade** | — | M (wide mechanical) | M |

**Hard ordering constraints:** A3 (ProtocolSettings) and B9 (neo-node-traits) gate everything above them. B5 (chain types into neo-p2p) must precede B6/B7/B10/B11 (they all depend on payload types). B4 (neo-vm) must precede B6.

---

## 4. RISKS & OPEN QUESTIONS FOR THE USER

**Top risks (consensus-critical, must be pure relocations + parity-tested):**
1. **`CallFlags` move (A1)** — serde/bit values gate CALLT/syscall permissions. Parity test required.
2. **`VerificationContext` wiring (A4)** — C#-parity ordering comments in `transaction/verification.rs` (Helper.cs:334-345) must be preserved verbatim; trait indirection must not reorder/short-circuit checks.
3. **`ProtocolSettings` unification (A3)** — two structurally divergent structs; committee/validators/hardfork heights are consensus. Verify vs C# v3.9.1 before deleting `neo-config/protocol.rs`.
4. **Native inversion (A6)** — 207 `&mut ApplicationEngine` sites on the execution path; staged co-location mitigates but this is the single largest seam.
5. **Duplicate Block collapse (B5)** — `ledger/block.rs` vs `payloads/block.rs` (confirmed both `pub struct Block`); `block_header.rs` has a lazy `_hash` Mutex cache. Diff `serialize()` byte-for-byte before merging.

**Open questions:**
- **Q1.** May the migration use a **transitional `neo-core` facade** (re-exports) so each step lands green without rewriting `neo-node`/`tests`/`neo-rpc` paths in the same commit, with facade deletion as the final step? (Recommended — the alternative is a much larger atomic path rewrite.)
- **Q2.** **ProtocolSettings**: confirm neo-core's typed struct (`standby_committee: Vec<ECPoint>`) is canonical and `neo-config`'s string-based one can be deleted after parity check — or must both shapes survive (JSON-load vs runtime)?
- **Q3.** **Wallets placement (S9)**: accept forward edge `neo-wallets → neo-smart-contract` (simpler, already acyclic), or invest in the `VerificationScript` trait to keep wallets below the engine? Recommend accepting the edge unless a plugin needs wallets below smart-contract.
- **Q4.** **`neo-network` naming**: extend the existing `neo-p2p` to also hold transport, or a separate `neo-network` crate above neo-p2p? Recommend **separate** (neo-p2p = types/codecs, neo-network = transport/actors) to keep neo-p2p dependency-light for plugins.
- **Q5.** **LZ4 compression**: neo-core's top-level `compression` mod is wire-relevant (ExtensiblePayload). Fold into `neo-io` (recommended) or a tiny `neo-compression`?
- **Q6.** **`neo-vm` rename collision**: the promoted host crate is `neo-vm` while the pure VM is `neo-vm-rs`. Confirm this naming (host=`neo-vm`, pure=`neo-vm-rs`) or pick alternatives to avoid confusion.

**Relevant paths:** seam drafts at `/Users/jinghuiliao/git/r3e/neo-rs/neo-primitives/src/verification.rs` (S3/S4), existing SystemContext seam at `/Users/jinghuiliao/git/r3e/neo-rs/neo-core/src/services/traits.rs` (S7/S8), CallFlags to move at `/Users/jinghuiliao/git/r3e/neo-rs/neo-core/src/smart_contract/call_flags.rs` (S1), divergent settings at `/Users/jinghuiliao/git/r3e/neo-rs/neo-config/src/protocol.rs` vs `/Users/jinghuiliao/git/r3e/neo-rs/neo-core/src/protocol_settings.rs` (A3), duplicate Block at `/Users/jinghuiliao/git/r3e/neo-rs/neo-core/src/ledger/block.rs` vs `/Users/jinghuiliao/git/r3e/neo-rs/neo-core/src/network/p2p/payloads/block.rs` (B5).

---
## Raw area analyses (evidence)
### vm-and-contracts
- internal_deps: FORWARD (clean): smart_contract -> neo_vm is the dominant legitimate edge (106 `crate::neo_vm` refs; aliased as `crate::vm_runtime`). neo_vm depends ONLY on neo-vm-rs (pure VM), neo_io, neo_primitives — it has NO dependency on ledger/network/persistence. smart_contract also depends on: crate::persistence (45), crate::network::p2p::payloads (Transaction/Signer/Witness/Header/ExtensiblePayload/Block ~33), crate::ledger (Block/BlockHeader, 7), crate::protocol_settings (24, dup of neo-config), crate::error (82), crate::script_builder/script_validation (7).

BACK-EDGE #1 (CYCLE neo_vm<->smart_contract): neo_vm -> smart_contract has EXACTLY 2 edges, both `use crate::smart_contract::CallFlags` (neo_vm/interop_service.rs:10, neo_vm/execution_engine/mod.rs:72). ExecutionEngine stores `call_flags: CallFlags` and InteropDescriptor stores `required_call_flags: CallFlags`. CallFlags is a pure bitflags type (only deps: bitflags + serde). This is the single cycle inside this area.

BACK-EDGE #2 (CYCLE persistence<->smart_contract): persistence/storage_item.rs:9-10 uses `crate::smart_contract::{BinarySerializer, interoperable::Interoperable}` while smart_contract uses crate::persistence (45 refs incl. StorageItem/StorageKey re-exported from smart_contract/mod.rs). Cross-area but rooted in serializer/Interoperable placement.

BIDIRECTIONAL native<->ApplicationEngine (intra-crate coupling, not a crate cycle if co-located): NativeContract trait (native/native_contract.rs:143-159) methods invoke/initialize/on_persist/post_persist all take `&mut ApplicationEngine`; conversely ApplicationEngine constructs/calls native concretes (application_engine/mod.rs:105-106 imports ContractManagement+others; witness_and_misc.rs:26,32 builds OracleContract/LedgerContract; load_execute_storage.rs:198 NativeHelpers; application_engine_contract.rs:11 Bls12381Interop). 207 `ApplicationEngine` mentions across native/.

DEPENDS-ON-THEM (downstream): crate::ledger depends on smart_contract (23 refs: ApplicationEngine, native::{LedgerContract,PolicyContract,GasToken,NeoToken,RoleManagement}, NativeHelpers) — ledger is strictly above smart_contract, no cycle. crate::persistence depends on smart_contract (serializer cycle above).
- proposed_crates: neo-vm (rename/promote of current neo_vm host), neo-smart-contract, neo-native-contracts, neo-native-traits (tiny seam crate)
- trait_seams: BREAK neo_vm<->smart_contract CYCLE (the only intra-area cycle): move `CallFlags` (pure bitflags, deps = bitflags+serde only) from smart_contract/call_flags.rs into neo-primitives. Then neo-vm's InteropDescriptor.required_call_flags and ExecutionEngine.call_flags reference neo_primitives::CallFlags, eliminating the 2 back-edges (interop_service.rs:10, execution_engine/mod.rs:72). reth precedent: shared scalar/flag enums live in reth-primitives, never re-imported upward from a higher crate. polkadot precedent: such primitives live in sp-core. | BREAK ApplicationEngine<->native bidirectional coupling: introduce a `NativeContract` trait (already exists in native/native_contract.rs) hoisted into `neo-native-traits` keyed against an `EngineHost`/`InteropContext` trait that ApplicationEngine implements, instead of native methods taking concrete `&mut ApplicationEngine`. Engine holds `Vec<Arc<dyn NativeContract>>` via a registry passed in at construction (dependency inversion) so neo-smart-contract does NOT import neo-native-contracts. This mirrors reth's EvmConfig/Handler-trait inversion (revm Inspector/Host traits) and substrate's pallet trait Config: the runtime wires concrete pallets; the executive depends on traits. NOTE: this is the highest-effort seam — 207 `&mut ApplicationEngine` call-sites in native/ must be re-typed to the host trait or kept concrete by co-locating, see risks. | KEEP existing ScriptContainer seam: ApplicationEngine already stores `Option<Arc<dyn Verifiable>>` (neo_primitives::Verifiable) NOT concrete Transaction (application_engine/mod.rs:221, state.rs). No new trait needed — VerifiableExt::as_transaction() downcasts when a concrete Transaction is required (helper.rs:370). This is the model seam; replicate its style for the native split. | BREAK persistence<->smart_contract CYCLE: persistence/storage_item.rs depends on smart_contract::{BinarySerializer, Interoperable}. Since serializers move to neo-vm (they only need StackItem) and `Interoperable` already lives in neo_vm/interoperable.rs (smart_contract just re-exports it), point persistence at neo-vm instead of neo-smart-contract — cycle dissolved. The `Interoperable` trait is the seam and it is correctly already in the VM-host layer.
- risks: "(1) HIGHEST RISK — native<->ApplicationEngine inversion: 207 `&mut ApplicationEngine` references across native/ and reciprocal engine->native constructions (mod.rs:105, witness_and_misc.rs:26/32, load_execute_storage.rs:198). Fully inverting to a host-trait is large and touches consensus-critical execution. SAFER STAGED PATH: first extract neo-vm (move serializers + CallFlags to primitives) and neo-smart-contract (engine framework) but TEMPORARILY keep ApplicationEngine + native in ONE crate (neo-smart-contract) so the bidirectional coupling stays intra-crate (legal, no cycle); split neo-native-contracts out only after the EngineHost trait seam lands. Do NOT split native first. (2) Protocol-parity: CallFlags serde repr and InteropDescriptor required-flags checks are consensus-observable (CALLT/syscall permission gates) — moving CallFlags must be a pure relocation, byte/enum-identical; add a parity test asserting flag values unchanged. (3) Serializer move: binary_serializer reproduces C# BinarySerializer limits (ExecutionEngineLimits) used by StdLib.serialize syscall — relocating to neo-vm must not change MaxItemSize/MaxStackSize behavior; native std_lib/tests.rs must still pass. (4) protocol_settings (24 refs) is a dup of neo-config ProtocolSettings — the engine should consume neo-config::ProtocolSettings; reconciling the dup is prerequisite to a clean neo-smart-contract dep list and is a separate de-dup task. (5) network::payloads dependency: neo-smart-contract + neo-native need Transaction/Signer/Witness/Block/Header/ExtensiblePayload — these must already be extracted into a neo-payloads crate (network area) BEFORE this split compiles; this is a hard cross-area ordering constraint. (6) vm_runtime/script_builder are lib.rs aliases — callers use `crate::vm_runtime` and `crate::script_builder`; update to the new crate paths during the move (mechanical but wide)."

### ledger-network-system
- internal_deps: EVIDENCE (crate:: edge counts):

(A) payloads -> smart_contract (12 edges, THE central cycle): ApplicationEngine::new(TriggerType::Verification,...) called directly in transaction/verification.rs:312 and header/verification.rs:164; LedgerContract::new().current_index(snapshot) in header.rs, extensible_payload.rs, conflicts.rs, not_valid_before.rs, transaction/verification.rs, oracle_response.rs (height lookup); ContractManagement::get_contract_from_snapshot + ContractBasicMethod::VERIFY + ContractParameterType::Boolean + CallFlags + Helper for contract-witness path. So payloads (chain types) reach UP into smart_contract (native contracts + VM engine).

(B) payloads <-> ledger (BIDIRECTIONAL back-edge): payloads -> ledger: HeaderCache (block/verification.rs:2, header/verification.rs:2), TransactionVerificationContext (transaction/verification.rs:18,39,54), VerifyResult (payloads/mod.rs:90 re-export). ledger -> payloads: 31 crate::network::p2p::payloads::* edges (Transaction, Block, Header, Witness, Signer in ledger/block.rs, block_header.rs, header_cache.rs imports payloads::Header, transaction_verification_context.rs imports payloads + smart_contract::native::GasToken, ledger_context.rs imports payloads). NOTE: ledger/block.rs+block_header.rs define a SEPARATE Block/BlockHeader from payloads::block::Block / payloads::header::Header (duplication / two parallel chain-type families).

(C) neo_system <-> ledger AND neo_system <-> network (BIDIRECTIONAL via NeoSystemContext god-handle): neo_system -> ledger (8), -> ledger::blockchain (5), -> network::p2p (14), -> smart_contract::native (12), -> smart_contract::application_engine (4). Back-edges INTO neo_system: ledger/blockchain/mod.rs:57 `use crate::neo_system::NeoSystemContext` (field system_context: Option<Arc<NeoSystemContext>>); network/p2p/task_manager.rs:31 + task_manager/handle.rs + task_manager/state.rs all `use crate::neo_system::NeoSystemContext`. Methods actors call on the context: store_cache(), settings()/protocol_settings(), ledger(), header_cache(), memory_pool()/memory_pool_handle(), state_store(), record_extensible(), broadcast_plugin_event(), event_stream(), is_fast_sync_mode(), self_ref(). So neo_system is NOT cleanly top-level today — blockchain & task_manager actors hold a back-reference to it.

(D) transport (network/p2p non-payload) -> payloads (correct direction): messages.rs references payloads::*; transport also -> ledger (5), neo_system (4), smart_contract (2). The ledger/neo_system/smart_contract edges from transport are the actor back-references (item C) plus a LedgerContract height read.

(E) witness.rs -> smart_contract::helper::Helper (witness.rs:42) + neo_vm_rs::OpCode + neo_crypto + neo_io. witness_rule core -> neo_io + neo_vm + constants only; witness_rule/stack_projection.rs -> crate::neo_vm::StackItem + neo_vm_rs::StackValue (the only VM-coupled witness_rule file).

(F) Existing-but-UNUSED seam: neo_primitives::verification.rs already defines `VerificationContext` trait (verify_witness(hash,&dyn Witness)->Result<bool>, get_gas_consumed/get_max_gas) AND a read-only snapshot trait, with doc comments literally saying "Transaction (in neo-p2p) can verify itself via trait methods; ApplicationEngine (in neo-core) implements this trait". neo_primitives::Verifiable also exists (re-exported as crate::Verifiable). neo-core/lib.rs:378 defines VerifiableExt: Verifiable but its verify_witnesses() hard-calls smart_contract::helper::Helper::verify_witnesses, so the seam is bypassed. payloads use `Arc<dyn crate::Verifiable>` as the engine container but still construct the concrete ApplicationEngine inline.
- proposed_crates: neo-ledger-types, neo-verification (traits crate; alternatively fold the traits into neo-primitives which already has them), neo-ledger, neo-network (or extend existing neo-p2p), neo-node-traits (tiny), neo-system (thin top crate)
- trait_seams: WITNESS-VERIFY SEAM (breaks payloads->smart_contract::ApplicationEngine, the central cycle). Concrete coupling: transaction/verification.rs:312 and header/verification.rs:164 call ApplicationEngine::new(TriggerType::Verification, container, snapshot, settings, gas). Trait: finish/use the ALREADY-DRAFTED neo_primitives::verification::VerificationContext (fn verify_witness(&self,&UInt160,&dyn Witness)->Result<bool>). Lives in neo-primitives (or neo-verification). neo-ledger-types payloads receive &dyn VerificationContext; the concrete impl (wrapping ApplicationEngine) lives in neo-smart-contract. This is exactly reth's approach: reth-primitives transaction types stay execution-free and the EVM (reth-evm/revm) implements execution behind a trait, never the reverse. The doc comment in verification.rs already states this intent — it just needs to be wired. | HEIGHT/SNAPSHOT SEAM (breaks payloads->smart_contract::native::LedgerContract). Concrete coupling: LedgerContract::new().current_index(snapshot) called in 6 payload files for current height during validity checks (ValidUntilBlock, NotValidBefore, Conflicts). Trait: HeightProvider/VerificationSnapshot (read-only) in neo-primitives — payloads call snapshot.current_index() instead of reaching into the native LedgerContract. Mirrors substrate sp-runtime traits (Block/Header) and reth's StateProvider: the read-only state view is a trait in a low crate, concrete DB/native-contract impl sits above. | LEDGER-TYPES vs LEDGER-ACTOR SEAM (breaks payloads<->ledger bidirectional). Concrete coupling: payloads import ledger::{HeaderCache, TransactionVerificationContext, VerifyResult} while ledger imports 31 payloads::* . Resolution is NOT a trait but a MOVE: relocate HeaderCache, TransactionVerificationContext, VerifyResult, LedgerContext (the pure data structures) DOWN into neo-ledger-types alongside the payloads they wrap; the blockchain ACTOR + mempool stay in neo-ledger above. After the move the edge is one-directional: neo-ledger -> neo-ledger-types. (substrate splits sp-blockchain/sp-runtime types from sc-client/sc-consensus services the same way.) | SYSTEM-CONTEXT INVERSION SEAM (breaks neo_system<->ledger and neo_system<->network). Concrete coupling: ledger/blockchain/mod.rs:57 and network/p2p/task_manager.rs:31 hold Arc<NeoSystemContext> and call ~12 methods on it. Trait: neo-node-traits::SystemContext exposing exactly those methods (store_cache/settings/ledger/header_cache/memory_pool/state_store/record_extensible/broadcast_plugin_event/event_stream/is_fast_sync_mode/self_ref). Actors hold Arc<dyn SystemContext>; NeoSystemContext (in top neo-system) impls it. This is dependency inversion — same pattern as reth's FullNodeComponents / NodeTypes traits that let pipeline stages reference the node without the node crate depending back on them. | WITNESS_RULE VM SEAM (isolates the one VM-coupled witness_rule file). Concrete coupling: witness_rule/stack_projection.rs uses crate::neo_vm::StackItem + neo_vm_rs::StackValue; the rest of witness_rule + witness.rs are VM-free except witness.rs:42 Helper. Since neo-ledger-types already depends on neo-vm-rs, StackValue is fine there; move the Helper-dependent witness signing/script helper into neo-ledger-types too (Helper's script-building subset has no native-contract dependency) — no extra trait needed, just keep neo-vm-rs (pure) as an allowed dep of neo-ledger-types.
- risks: RISK-1 (protocol parity, HIGH): The witness-verify and height seams sit on the consensus-critical verification path (Transaction.VerifyWitnesses ordering, Oracle empty-witness handling, MAX_VERIFICATION_GAS, validate_strict_script). The C#-parity ordering comments in transaction/verification.rs (Helper.cs:334-345) MUST be preserved verbatim when the body moves behind VerificationContext — a trait indirection that reorders or short-circuits checks changes consensus. Mitigation: the trait wraps the EXISTING ApplicationEngine::new call 1:1; move code, do not rewrite logic; keep p2p_message_tests + block/header/tx verification tests green as the gate.

RISK-2 (duplicate chain types, MEDIUM): ledger/block.rs+block_header.rs define Block/BlockHeader parallel to payloads::block::Block / payloads::header::Header. Collapsing them into one set inside neo-ledger-types is required for a clean graph but risks subtle field/serialization drift (block_header.rs has a lazy _hash Mutex cache; the payloads version may differ). Mitigation: diff the two structs byte-for-byte on serialize() before merging; this is a behavior-preserving requirement, not a guess.

RISK-3 (the unused seam may have rotted, MEDIUM): neo_primitives::VerificationContext exists with zero non-test impls/dyn-uses found in neo-core/neo-p2p, so its signature may not match current ApplicationEngine usage (e.g. it takes &dyn Witness but payloads pass settings+snapshot+gas explicitly). Expect to extend the trait (add snapshot/settings/gas params or a richer factory) rather than use as-is.

RISK-4 (NeoSystemContext surface, MEDIUM): the SystemContext trait must capture genesis_block, ledger, state_store, service_registry access without leaking concrete Arc<LedgerContext>/Arc<StateStore> back into low crates — those return types themselves must live in neo-ledger-types/neo-storage or the inversion fails. Audit every method's return type for an upward concrete leak before finalizing.

RISK-5 (persistence/neo-storage overlap, LOW-MEDIUM): payloads use crate::persistence::{DataCache, StoreCache}, which mod.rs already re-exports from neo_storage. Ensure neo-ledger-types depends on neo-storage directly (not neo-core's persistence shim) so DataCache is the single canonical type; persistence/ has 2-3 stray smart_contract edges (Interoperable) to resolve separately.

RISK-6 (sequencing/big-bang, MEDIUM): payloads + ledger-types + traits must move together because of the bidirectional edge; this is one large atomic extraction, not incremental. Mitigation: land neo-node-traits (SystemContext) and the verification traits FIRST as additive (actors switch Arc<NeoSystemContext> -> Arc<dyn SystemContext> in place, still in neo-core), prove green, THEN physically split crates.

### services-and-peripherals
- internal_deps: EVIDENCE from `use crate::` grep across each module.

== The 4 inlined C# plugins (all are LEAF consumers — nothing in neo-core depends back on them; confirmed `grep -rl` shows zero reverse edges) ==
oracle_service → network::p2p::payloads (OracleResponse/OracleResponseCode/Transaction + helper::get_sign_data_vec) [28x], smart_contract (ApplicationEngine, Contract, native::{OracleContract,LedgerContract,Role,RoleManagement,NativeContract}, StorageItem, CallFlags) [17x], wallets (Wallet, KeyPair) [16x], persistence (DataCache, StoreCache, StorageKey) [8x], neo_io [4x], protocol_settings [3x], neo_system::NeoSystem [3x]. External: tonic/prost/reqwest/futures (NeoFS gRPC + HTTPS oracle), neo_crypto [9x].
state_service → smart_contract (StorageItem/StorageKey, Contract, ContractParametersContext, native::{RoleManagement,LedgerContract,Role,NativeContract}) [15x], persistence (DataCache, Store, StoreCache, StoreSnapshot, ReadOnlyStore, MemoryStoreProvider, WriteStore, TrackState) [14x], neo_io [7x], protocol_settings [4x], network::p2p::payloads (ExtensiblePayload, Witness + helper) [4x], wallets [3x], ledger (Block, BlockHeader, ApplicationExecuted, BlockchainCommand/PersistCompleted/RelayResult/VerifyResult) [3x], i_event_handlers [1x].
tokens_tracker → neo_io [11x], smart_contract (ApplicationEngine, TriggerType, CallFlags, native::ContractManagement, StorageItem/StorageKey) [8x], persistence (DataCache, Store, SeekDirection, StoreSnapshot) [7x], neo_ledger (ApplicationExecuted, Block) [4x], neo_vm::StackItem [3x], extensions [3x], script_builder [2x], i_event_handlers [1x].
application_logs → smart_contract (NotifyEventArgs, TriggerType, StorageItem/StorageKey) [2x], persistence (DataCache, Store, StoreSnapshot) [2x], ledger (Block, ApplicationExecuted) [2x], neo_vm::StackItem, neo_system::NeoSystem, rpc_json (stack_item serializers), i_event_handlers (CommittedHandler/CommittingHandler), UInt256, UnhandledExceptionPolicy.

== Peripheral utilities ==
wallets → smart_contract (Contract, ContractParameterType, helper::Helper) [5x] — THIS IS THE NOTABLE BACK-EDGE: wallet signing-context construction needs smart_contract::Contract; network::p2p::payloads (Transaction, Witness, Signer, HEADER_SIZE) [5x], protocol_settings [3x], script_builder [2x], constants [2x], big_decimal, neo_io. bip32 submodule is self-contained (key_path/extended_key).
persistence → re-export shim over neo-storage: `pub use neo_storage::StorageItem`, imports `neo_storage::types::storage_item::CacheProvider`. Adds StorageItemExt trait (cache-aware, depends on smart_contract::{BinarySerializer, Interoperable} + neo_vm_rs::ExecutionEngineLimits + num_bigint) [3x smart_contract] and the rocksdb provider (providers/rocksdb/{provider,store}.rs, 1,100 LOC, depends on `rocksdb` crate). neo_io [3x], error [3x].
actors → ZERO neo coupling confirmed (grep `use crate::` empty). External only: tokio, tokio-util (CancellationToken/TaskTracker), async-trait, dashmap, parking_lot, thiserror, uuid. Pure generic tokio actor runtime.
telemetry → ZERO neo coupling confirmed. External only: dashmap, parking_lot, tracing. Dup of neo-telemetry.
builders → network::p2p::payloads (Transaction, Witness, Signer, WitnessRule/WitnessRuleAction, WitnessCondition) [5x], error::CoreError [1x]. Pure fluent builders over payload types.
services → traits.rs defines LedgerService/StateStoreService/MempoolService/PeerManagerService/RpcService/SystemContext (THE existing trait seams). mod.rs wires concrete LockedMempoolService over crate::ledger::MemoryPool + crate::state_service::StateStore. SystemContext imports persistence::StoreCache, protocol_settings::ProtocolSettings, smart_contract::{ApplicationEngine,LogEventArgs,NotifyEventArgs}.
extensions → compression (compress_lz4/decompress_lz4 from neo-core's top-level mod.rs `compression`), error::CoreError. Byte/Span/Memory/IO extension traits.
events → handlers.rs couples to ledger::{Block, ApplicationExecuted}, network::message::Message, wallets::Wallet, error::CoreResult, uses `&dyn Any` for the system param (partial seam). Re-exported as crate::i_event_handlers — consumed by all 4 plugins + state_service.
properties → ZERO crate edges. AssemblyInfo version strings only.
protocol_settings.rs → imports neo_crypto::ECPoint, crate::Hardfork. 59 in-tree consumers. neo-config::ProtocolSettings has 0 consumers in neo-core.
witness_rule → constants, neo_vm (StackItem projection) [2x].
- proposed_crates: neo-node-traits, neo-oracle-service, neo-state-service, neo-tokens-tracker, neo-application-logs, neo-actors, neo-storage-rocksdb (or fold into neo-storage as optional feature), neo-wallets, neo-tx-builder, MERGE: telemetry → neo-telemetry, MERGE: protocol_settings.rs → neo-config (canonical) , FOLD: extensions / events-concrete / properties
- trait_seams: EXISTING seam, reuse it: neo-core/src/services/traits.rs already defines SystemContext + LedgerService/StateStoreService/MempoolService/PeerManagerService/RpcService. SystemContext::{store_cache, protocol_settings, current_block_index, block_hash_at, mempool_count, notify_application_log/notify} is the runtime-abstraction that lets plugins avoid depending on neo_system/ledger concretes. Promote this file verbatim into a new tiny `neo-node-traits` crate (object-safe, no heavy deps). Concrete impl stays in neo-node. This is reth's NodeComponents/FullNodeComponents pattern and substrate's sp-api runtime-trait split. | Plugin event seam: crate::events::handlers (CommittedHandler/CommittingHandler/MessageReceivedHandler/WalletChangedHandler), re-exported as i_event_handlers and consumed by all 4 plugins + state_service. Today these traits embed concrete Block/Message/Wallet and `&dyn Any` for system. Move the trait definitions into `neo-node-traits` alongside SystemContext; keep the concrete arg types (Block/Message) since they live in lower crates (neo-ledger-types/neo-p2p) the plugins already depend on. Replace `&dyn Any` with `&dyn SystemContext`. This is substrate's event-handler/`OnFinalize` hook pattern — hooks defined in a traits crate, dispatched by the orchestrator (neo-node). | persistence back-edge seam: StorageItemExt depends on smart_contract::{Interoperable, BinarySerializer}. neo-storage already exposes a `CacheProvider` trait that StorageItemExt's StorageCache enum implements — that IS the seam. Keep StorageItemExt OUT of neo-storage; relocate it next to the smart-contract VM-interop crate (neo-smart-contract) as an extension trait on neo_storage::StorageItem. neo-storage stays leaf; the smart_contract-coupled cache logic lives where Interoperable lives. reth does this: reth-db holds the raw codec, reth-provider/evm holds the typed-decode extensions. | wallets→smart_contract::Contract back-edge: WalletAccount signing-context needs the multi-sig/single-sig Contract verification script. Introduce a narrow `VerificationScript`/`ContractScriptProvider` trait (the bytes + script-hash) in neo-primitives (which already hosts Verifiable, UInt160/256) and have wallets depend on the trait, not on the full smart_contract::Contract. Mirrors neo_primitives::Verifiable already used to break the witness/verification cycle. Alternatively accept wallets→neo-smart-contract as a forward edge if Contract lands in a mid-layer crate below wallets (acyclic either way; trait only needed if wallets must sit below smart-contract).
- risks: PROTOCOL-PARITY RISK (HIGH) — protocol_settings.rs: do NOT naively 'merge into neo-config'. The two structs are STRUCTURALLY DIVERGENT and consensus-relevant. neo-core's uses typed `standby_committee: Vec<ECPoint>` + `Hardfork` enum keys + `validators_count`; neo-config's uses `standby_validators: Vec<String>` + string hardfork keys + flat per-native activation-height fields. All 59 in-tree consumers use neo-core's; neo-config's has 0 consumers. Committee membership, validators_count, milliseconds_per_block, max_traceable_blocks, and hardfork heights all drive consensus/wire behavior. SAFE PLAN: make neo-core's ProtocolSettings the canonical one, move it into neo-config (or a new neo-protocol-settings crate that neo-config re-exports), DELETE the divergent neo-config::protocol.rs struct, and convert its JSON-loading fields by mapping — but verify byte-for-byte against C# Neo v3.9.1 ProtocolSettings before deleting either. This is the single highest-risk item in this area.

MERGE RISK (MEDIUM) — persistence→neo-storage: most of persistence/ is already a re-export shim over neo-storage (StorageItem/Key/DataCache/TrackState/SeekDirection are canonical in neo-storage). The genuinely neo-core-specific pieces are (a) StorageItemExt (smart_contract-coupled — must NOT go into leaf neo-storage; see trait seam), and (b) the rocksdb provider (~1,100 LOC, pulls the `rocksdb` crate). Putting rocksdb into neo-storage forces every neo-storage consumer to pull rocksdb; prefer a separate `neo-storage-rocksdb` backend crate (reth-db-rocksdb pattern) implementing neo-storage's provider trait. Risk: subtle serialization (get_var_size_bytes) parity if StorageItem encode/decode is touched during the move.

LOW RISK — actors→neo-actors and telemetry→neo-telemetry are clean lifts (zero neo coupling verified). builders, extensions, properties are pure utilities. Main caution: `extensions` pulls neo-core's top-level `compression` (LZ4) module — that compression mod must move with/before extensions (candidate: fold compress_lz4/decompress_lz4 into neo-io or a tiny neo-compression crate, since witness/payload LZ4 is wire-relevant for ExtensiblePayload).

ORDERING: protocol_settings unification and the events/SystemContext traits crate are prerequisites — every plugin and state_service transitively needs them, so extract neo-node-traits + canonicalize ProtocolSettings FIRST, then the 4 plugin crates can be lifted independently and in parallel.


---
## USER DECISIONS (2026-05-30) — locked
- **Migration style: BIG-BANG ATOMIC (no transitional neo-core facade crate).** Repoint consumers to the real crates; single-type re-exports during a move are fine, but no whole-crate facade. End state: neo-core deleted.
- **P2P: SPLIT — neo-p2p (types/codecs, dependency-light) + neo-network (transport/actors above it).**
- **ProtocolSettings: neo-core's typed struct → neo-config canonical; DELETE neo-config/src/protocol.rs after byte-for-byte C# v3.9.1 verification.**

## EXECUTION PROGRESS
- [x] A1 — CallFlags → neo-primitives (seam S1, parity test). Commit 07b8728d. Breaks neo_vm↔smart_contract cycle.
- [ ] A2 — persistence↔smart_contract cycle (move BinarySerializer/Interoperable usage to neo-vm path).
- [ ] A3 — ProtocolSettings unification (neo-config canonical) — HIGH risk, consensus; verify vs C# first.
- [ ] A4 — VerificationContext/VerificationSnapshot wiring (seams S3/S4).
- [ ] A5 — SystemContext inversion in-place (seam S7/S8).
- [ ] A6 — NativeContract/EngineHost seam (S6) — XL, 207 sites.
- [ ] B1–B14 — physical extraction leaves-first (actors, telemetry, storage-rocksdb, vm, p2p-types, smart-contract, natives, wallets, node-traits, ledger, network, plugins, system, delete neo-core).


## Execution progress (updated 2026-05-30)

GREEN commits landed this effort:
- A1 (07b8728d) CallFlags -> neo-primitives (breaks vm<->smart_contract cycle)
- B1 (57e63dd4) neo-actors extracted (standalone crate; neo-rpc/neo-node repointed)
- B2 (3f56b32e) telemetry facade consolidated into neo-telemetry
- (cd65e391) redundant dashmap metrics facade retired; node on existing prometheus stack
- A3 (2d1aaed1) **GATE DONE** typed ProtocolSettings canonical in neo-config; string dup
  deleted; HardforkManager moved down; neo-core re-exports; no cycle. Eliminates the
  ProtocolSettings duplication. neo-core now ~97.3K LOC (from ~102K).

Remaining gates/seams (each entangled, sequence carefully):
- A2 persistence<->smart_contract (StorageItemExt via Interoperable/BinarySerializer)
- A4 VerificationContext wiring; A5 SystemContext inversion; A6 NativeContract/EngineHost
- B3 neo-storage-rocksdb (needs A2/S2) ; B4 neo-vm ; B5 neo-p2p chain types (big)
- B9 neo-node-traits GATE: blocked — SystemContext leaks concrete StateStore/ledger/
  StoreCache/smart_contract types that must move to low crates (B5/B10) first.
- B6/B7/B10/B11 depend on B5 ; B12 plugins depend on B9 ; B13 neo-system ; B14 delete neo-core.

Infra fold-ins (reth/polkadot strategy, claudedocs/reth-polkadot-reuse-strategy.md):
- metrics+exporter swap: turnkey recipe written (multi-crate: neo-telemetry + neo-rpc).
- MDBX backend behind neo-storage Store traits: with B3.
- jsonrpsee default for neo-rpc: with the RPC layer work.

- A2/S2 (dd913193) **SEAM DONE** persistence->smart_contract back-edge broken: StorageItemExt
  + StorageCache (need BinarySerializer/Interoperable) moved to smart_contract/storage_item_ext.rs;
  persistence/storage_item.rs now a clean neo_storage::StorageItem re-export; rocksdb provider
  sources StorageItem/StorageKey from persistence not smart_contract; orphan
  smart_contract/storage_item.rs deleted. persistence/ now has ZERO edges into smart_contract/neo_vm.

B3 reality check (2026-05-30): the rocksdb provider (1336 LOC) depends on the Store-trait layer
(Store/ReadOnlyStore/WriteStore/StoreSnapshot/StoreProvider/SeekDirection/StorageConfig/read_cache/
write_batch_buffer) which STILL lives in neo-core/persistence, not neo-storage. So B3 = (1) move
that trait layer down into neo-storage, THEN (2) extract neo-storage-rocksdb as a backend crate.
Medium-large, not a quick lift. B4 (neo_vm ~12.8K LOC) similarly large.


## Corrected storage-layer state (2026-05-30, verified)

The plan assumed "Store-trait layer still in neo-core/persistence" — WRONG. Verified current state:
- The Store traits ARE ALREADY in neo-storage: `neo-storage/src/persistence/{store,read_only_store,
  write_store,store_snapshot,store_provider}.rs` + read_cache. neo-core/persistence/mod.rs is mostly a
  re-export shim (12x `pub use neo_storage`). The neo-core/persistence/store.rs etc. files DON'T EXIST.
- neo-storage has its OWN error: `StorageError` / `StorageResult` (neo-storage/src/error.rs).
- Still in neo-core (must move for B3): `write_batch_buffer.rs` (actually ROCKSDB-specific — uses
  rocksdb::{WriteBatch,WriteOptions,DB} + crate::CoreError) and `storage.rs` (StorageConfig, uses
  crate::error::CoreResult). The rocksdb provider (providers/rocksdb/, 1336 LOC) couples to crate::error::
  {CoreError,CoreResult}.

**Precise B3 (neo-storage-rocksdb extraction), turnkey:**
1. New crate `neo-storage-rocksdb`, deps: neo-storage, neo-io, rocksdb, parking_lot, tracing.
2. git mv neo-core/src/persistence/providers/rocksdb/* + write_batch_buffer.rs into it.
3. Move storage.rs (StorageConfig) into neo-storage proper (it's generic config, not rocksdb-specific);
   swap CoreResult -> StorageResult.
4. In the moved provider + write_batch_buffer: swap crate::error::{CoreError,CoreResult} ->
   neo_storage::{StorageError,StorageResult} (map error variants).
5. neo-core/persistence re-exports the provider from neo-storage-rocksdb (back-compat) OR repoint
   neo-core's RocksDBStoreProvider consumers (neo-node, examples) to the new crate.
6. Verify: workspace --all-targets + neo-node + the persistence/storage tests.
This is the seam where the reth-libmdbx fold-in lands (a second provider crate behind the same
neo-storage Store traits). Medium effort, ~4-6 files + error-variant mapping.


## B3 extraction — TURNKEY recipe (2026-05-30, provider fully decoupled)

Prereqs DONE this session (committed): ba5892fe (provider error -> neo_storage::StorageError),
8cc96b1b (StorageConfig -> neo-storage). The RocksDB provider now depends ONLY on neo-storage +
rocksdb + parking_lot + tracing (+ tempfile dev). All consumers reach it via
`neo_core::persistence::providers::{RocksDBStoreProvider, rocksdb::*}` (neo-core/src/persistence/
providers.rs:10 `pub mod rocksdb;` + :13 `pub use rocksdb::RocksDBStoreProvider;`). write_batch_buffer
has NO external consumers (only the provider + dead neo-core re-exports).

Steps (each subtle point noted):
1. New crate `neo-storage-rocksdb/` — Cargo.toml deps: neo-storage{workspace}, rocksdb, parking_lot,
   tracing; dev: tempfile. Add to root Cargo.toml members + workspace.dependencies (version 0.7.2).
   CHECK FIRST: whether neo-core gates rocksdb behind a feature — if so, the new crate is always-on and
   neo-core's dep on it must carry that feature gate (or make neo-storage-rocksdb optional in neo-core).
2. git mv providers/rocksdb/{provider,store,tests}.rs + persistence/write_batch_buffer.rs into
   neo-storage-rocksdb/src/. Delete providers/rocksdb/mod.rs.
3. src/lib.rs: `pub mod provider; pub mod store; mod write_batch_buffer; #[cfg(test)] mod tests;` +
   the exact re-exports from the old providers/rocksdb/mod.rs (BatchCommitConfig.. RocksDBStoreProvider,
   RocksDbStore, RocksDbSnapshot) + write_batch_buffer types.
4. Import rewrites in moved files (ORDER MATTERS — subtle paths):
   a. `crate::persistence::write_batch_buffer` -> `crate::write_batch_buffer` (now crate-local)
   b. `crate::persistence::{StorageItem, StorageKey}` -> `neo_storage::{StorageItem, StorageKey}`
      (StorageItem/Key are at neo_storage ROOT, NOT neo_storage::persistence — blanket rewrite breaks this)
   c. remaining `crate::persistence::X` (seek_direction/storage/write_store/StoreCache/store_snapshot/
      store_provider/store/read_only_store/read_cache) -> `neo_storage::persistence::X`
   (write_batch_buffer.rs already uses neo_storage::{StorageError,StorageResult} from ba5892fe.)
5. neo-core: providers.rs -> `pub use neo_storage_rocksdb as rocksdb; pub use neo_storage_rocksdb::
   RocksDBStoreProvider;`. persistence/mod.rs: remove `pub mod write_batch_buffer;` (:19) + its re-exports
   (:41-42). neo-core/Cargo.toml: add neo-storage-rocksdb dep.
6. Verify: workspace --all-targets + neo-node + the ~8 neo-core mainnet_block_*_repro tests that use
   RocksDBStoreProvider (paths preserved by the re-export, so they should need NO changes).
This is the seam where reth-libmdbx lands as a 2nd `neo-storage-mdbx` crate behind the same Store traits.


## S3 seam — exact code-level state (2026-05-30, verified)

The `BlockchainSnapshot` seam trait ALREADY EXISTS in neo-primitives
(neo-primitives/src/verification.rs:201) with height/get_storage/contains_transaction/
contains_block/get_block_hash — BUT it has ZERO impls and ZERO usage (defined-but-unwired).
`VerificationContext` (same file:146) exists too (verify_witness + gas), also for the seam.

The chain-type attributes couple to smart_contract via their consensus-critical verify():
- conflicts.rs -> LedgerContract.contains_transaction(snapshot, hash)  [covered by BlockchainSnapshot]
- not_valid_before.rs -> LedgerContract (height)  [covered]
- transaction_attribute.rs (base) dispatch is UNIFORM: verify(settings, snapshot:&DataCache, tx)
  for all 5 attributes -> can't decouple one without changing the shared signature.
- HighPriority -> NativeHelpers (committee)  [NOT covered by BlockchainSnapshot]
- NotaryAssisted -> smart_contract::Helper  [NOT covered]
- TransactionAttribute.calculate_network_fee -> PolicyContract.get_attribute_fee  [separate method, NOT covered]

So S3 = (1) extend BlockchainSnapshot (or add a NativeQueries trait) to cover committee + policy +
helper queries; (2) create the concrete impl in neo-core (wrapper around DataCache, backed by the
native contracts — orphan rule means it must be a neo-core wrapper type, not impl-for-DataCache);
(3) change the uniform attribute verify() signature DataCache -> &dyn (snapshot+native) and route all
5 attributes through it; (4) update Transaction::verify (the caller) to construct + pass the impl.
This is consensus-path work (verify() is consensus-critical) — must be behavior-preserving and
differential-tested. ONLY THEN can the chain-type payloads move to neo-p2p (B5), which then unblocks
B9 (neo-node-traits) and the plugin/VM/smart_contract extractions. The seam infra exists; wiring it
is the intricate, ordered, consensus-adjacent next step.

## Verified frontier refresh (2026-05-31)

Re-measured the actual crate graph this session. Corrections to the checklist above:

- **B3 (neo-storage-rocksdb) is DONE** — the `turnkey recipe` sections above are stale. The
  crate exists, is a workspace dep, and is feature-gated: `neo-core/Cargo.toml` has
  `neo-storage-rocksdb = { workspace = true, optional = true }` + `rocksdb = ["dep:neo-storage-rocksdb"]`;
  `neo-core/src/persistence/providers.rs` re-exports it under the original
  `neo_core::persistence::providers::{rocksdb, RocksDBStoreProvider}` path (cfg `rocksdb`). The
  `providers/rocksdb/` source dir is gone from neo-core. No action left for B3.

- **✅ B4 (extract neo_vm → `neo-vm` crate) is DONE (2026-05-31, commit acbffe14).** A committed
  guard test (`tests/tests/no_local_neo_vm_dependency.rs`) originally FORBADE a local VM crate
  (design intent: pure VM = external `neo-vm-rs`; stateful host stays in neo-core). That conflict was
  surfaced to the user, who chose **"Revise design, extract it"** — consciously overriding the guard.
  So the extraction was executed AND the guard test updated to the new design (its first assertion
  now verifies the `neo-vm` host crate exists and builds ON `neo-vm-rs`, rather than forbidding it;
  source-inspection paths repoint to `neo-vm/src`). neo_vm (41 files, 12.8K LOC) moved out with zero
  back-edges; neo-core re-exports it as `neo_core::neo_vm` (`pub use ::neo_vm`) so all ~106 consumers
  + downstream crates are unchanged. Verified: neo-vm builds standalone, neo-core builds, workspace
  `--all-targets` builds, neo-core runtime suite fully green. NOTE: the guard-test binary has **5
  pre-existing failures + 2 unused-fn warnings unrelated to B4** (stale paths from the earlier
  oracle_service/call_flags extractions — confirmed identical on the pre-B4 baseline via git-stash).
  Those are separate test-debt to fix. See [[two-tier-vm-architecture]] (now: host = `neo-vm` crate,
  pure = `neo-vm-rs`).

- (Historical) earlier measurement of `neo-core/src/neo_vm` (41 files, 12,797 LOC) that justified B4:
  - **Zero back-edges**: no `crate::smart_contract|ledger|network|persistence|neo_system|state_service|wallets|services|builders` anywhere in the tree.
  - **Near-zero in-core deps**: the only `crate::` tokens in the whole subtree are `crate::neo_vm` (128 self-refs) and `crate::IoError` (6 sites — a neo-core lib-root re-export of neo-io's `IoError`; repoint to `neo_io::IoError` in the moved crate). No `crate::error::CoreError`, no `crate::constants`, no uppercase type re-exports (UInt160/Verifiable/etc.) — verified with a case-sensitive `crate::[A-Za-z_]` scan.
  - **Own error type**: `neo_vm/error.rs` defines `VmError`/`VmResult` (line 45/640) — NOT `CoreError`. So no shared-error coupling.
  - External deps only: `neo-vm-rs`, `neo-io`, `neo-primitives` (`neo_primitives::CallFlags`, 2 sites, post-A1).
  - It is effectively already a crate inside neo-core. Risk is mechanical size, not architecture.

  **Precise B4 recipe (path-preserving, consumers untouched):**
  1. New crate `neo-vm/` (host VM; pure VM stays `neo-vm-rs`). Cargo.toml deps: neo-vm-rs, neo-io, neo-primitives (+ serde/num-bigint/tracing as the tree uses). Add to root members + workspace.dependencies.
  2. `git mv neo-core/src/neo_vm/* neo-vm/src/`; the subtree's internal `super::`/relative refs stay valid.
  3. Rewrite internal refs in the moved files (single pass each): `crate::neo_vm::` → `crate::`, then `crate::IoError` → `neo_io::IoError`. Grep for any bare `crate::neo_vm` standalone-path leftovers and fix by hand.
  4. neo-core `lib.rs`: replace `pub mod neo_vm;` (324) with `pub use neo_vm as neo_vm;` (or `pub extern crate`/re-export) so `crate::neo_vm::X` keeps resolving for the ~106 consumer refs. Re-plumb the aliases: `pub mod vm_runtime;` (328 — confirm whether it is a `pub use neo_vm ...` alias), `pub use crate::neo_vm::rpc_json;` (251), and `pub mod script_builder;` (257 — confirm whether it lives inside neo_vm or is separate before moving).
  5. neo-core/Cargo.toml: add the `neo-vm` dep.
  6. Verify: build `neo-vm` alone → neo-core → `--workspace --all-targets` → the `runtime`-gated suites. Consumers should need no edits (path-preserving). If it can't land green, `git checkout` aborts cleanly.

  After B4, the remaining gated core is unchanged: **A4/S3 (verification-context seam)** is the
  intricate consensus-adjacent all-or-nothing work (see below), and **B5 (chain types → neo-p2p)**
  depends on it. A5 (SystemContext inversion in-place) and A6 (NativeContract/EngineHost) remain the
  other large seams. None of those is a quick lift; B4 is the one bounded, de-risked extraction ready to go.

### S3 per-attribute verify() query surface (proven 2026-05-30)
- conflicts.verify -> LedgerContract.contains_transaction      [BlockchainSnapshot covers]
- not_valid_before.verify -> LedgerContract.current_index       [BlockchainSnapshot covers (=height)]
- high_priority.verify -> NativeHelpers.committee_address(settings, snapshot)  [NEEDS committee query added]
- oracle_response.verify -> OracleContract.get_request + RoleManagement.get_designated_by_role_at
      [NEEDS Oracle+Role queries; these RETURN smart_contract domain types (OracleRequest, ECPoint set)
       -> the seam trait can't live in neo-primitives unless those domain types also move to a low crate]
- notary_assisted.verify -> snapshot UNUSED
CONCLUSION: S3 is all-or-nothing (uniform verify signature) and the OracleResponse branch forces moving
smart_contract domain types (OracleRequest, RoleManagement designation) into a low crate FIRST. So the
true ordering is: move Oracle/Role domain types down -> design the NativeQueries+BlockchainSnapshot seam
-> wire all 5 verify() -> update Transaction::verify caller -> differential-test on the consensus path
-> THEN B5 (chain types -> neo-p2p). This is the intricate, consensus-adjacent multi-session core.
