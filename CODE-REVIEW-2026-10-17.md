# Neo N3 Rust Full Node — Independent Code Review

**Date:** 2026-10-17  
**Repository:** `D:\Giteo-rs` (commit `46db40a2` + working tree)  
**Target:** Neo N3 v3.10.1 (C# reference), Neo.VM v3.10.1, DBFTPlugin 3.10.x, StateService/MPTTrie  
**Reviewer:** DeepSeek Harness coding agent  
**Scope:** architecture, protocol correctness/completeness, consensus safety, VM semantics/gas, P2P, storage/state root, Rust quality, performance claims

---

## 1. Verdict

This is a large, real implementation (≈300k lines of Rust across 16 crates) with substantial protocol coverage: byte-compatible block/transaction serialization, a working VM, native contracts, dBFT, P2P, RocksDB persistence, JSON-RPC, wallets, state service, and MPT. It is now a credible testnet/private-network node; "mainnet-ready" still requires the external differential campaign described in §4, which cannot be replaced by unit tests.

The review found and fixed **more than 20 critical/high-severity defects**, including remote crash/DoS paths, a block-persistence path that could commit unverified blocks, a VM stack-limit bypass, consensus wire-format divergence, and dBFT safety gaps. After the fixes:

- `cargo check --workspace` (library targets) succeeds **warning-free**; a few legacy test files still emit style warnings (unused imports/variables, naming), which do not affect runtime.
- `cargo test --workspace --no-fail-fast`: **4,517 passed, 0 failed** across 220 suites (2 ignored; including all 121 architecture-conformance tests).
- Before the review the same suite had **4,475 passed / 16 failed** (10 failures were real or stale tests fixed during this review; see §6).
- The remaining work is **validation** (C# differential replay/state roots, multi-node dBFT cluster, native-contract execution vectors), not known code defects. See §4.

The implementation is suitable for testnet/private-network consensus and for an RPC/sync node now; do not call it certified for mainnet until the differential campaign in §4 has been run and published.

---

## 2. What was reviewed

- **Crates:** `neo-primitives`, `neo-config`, `neo-crypto`, `neo-storage`, `neo-io`, `neo-json`, `neo-vm`, `neo-core`, `neo-p2p`, `neo-rpc`, `neo-consensus`, `neo-node`, `neo-telemetry`, `neo-hsm`, `neo-tee`, `tests`.
- **Differential references** (downloaded during review):
  - C# Neo `v3.10.1`: `%TEMP%eo-ref`
  - Neo.VM `v3.10.1`: `%TEMP%eo-vm-ref`
  - Neo plugins (DBFT/StateService/MPTTrie): `%TEMP%eo-plugins-ref`
  - Exact 3.10.1 DBFT plugin: `%TEMP%eo-node-3101-ref`
- **Automated:** workspace build, `cargo test`, targeted consensus/VM/native/P2P tests, exhaustive opcode-price comparison, one C# MPT root vector.
- **Manual:** block receive/verify/persist path, VM jump table and fast interpreter, native-contract metadata/hashes, dBFT messages/state machine, P2P framing/task manager, state store/StateRoot/MPT, feature flags and crate layout.
- **Not exhaustively reviewed:** every RocksDB option, every native-contract method body, every RPC response shape, and all 85 `unsafe` blocks in `neo-core` / 76 in `neo-vm`. These remain residual risk.

---

## 3. Critical and high-severity findings (fixed)

### C1. Unverified future blocks were persisted
**Where:** `neo-core/src/ledger/blockchain/block_processing.rs`, `neo-core/src/ledger/blockchain/mod.rs`  
**Problem:** `on_new_block` parked blocks with `index > header_height + 1` in `_block_cache_unverified` without verifying them. When the gap filled, the drain path called `persist_block_via_system` directly; `NeoSystem::persist_block` executes and commits state without verifying the header witness, previous hash, or index. A peer could inject an arbitrary future block and have it committed once the chain caught up. C# re-injects cached blocks through `OnNewBlock`, which verifies before persist.  
**Fix:** Added `Blockchain::verify_block_for_persistence` (`mod.rs:224`). Every block taken from the unverified/hash caches is matched against a witness-verified cached header, or fully verified against persisted state, before persistence. The drain paths now refuse mismatches and try other fork candidates. Regression tests cover both the matching and mismatching header cases.  
**Evidence:** `neo-core/src/ledger/blockchain/mod.rs:224`, `block_processing.rs:270-315`; tests in `ledger/blockchain/tests.rs`.

### C2. VM `MaxStackSize` was never enforced
**Where:** `neo-vm/src/execution_engine/execution.rs`  
**Problem:** After an instruction, the engine called `check_zero_referred()` and **discarded the result**. The comment said the check was “temporarily relaxed”. C# faults when `ReferenceCounter.Count > MaxStackSize` (2,048). A `PUSH1; JMP` loop could therefore grow memory without bound, and scripts C# FAULTs ran to completion (consensus divergence).  
**Fix:** `post_execute_instruction` now enforces `count > max_stack_size` after zero-reference collection; added a 2,049-push regression test.  
**Evidence:** `neo-vm/src/execution_engine/execution.rs:244`; test `max_stack_size_is_enforced_after_execution`.

### C3. `NEWARRAY` / `NEWARRAY_T` / `NEWSTRUCT` could request an arbitrary allocation
**Where:** `neo-vm/src/jump_table/compound.rs` → `neo-vm/src/semantics/collections.rs`  
**Problem:** The jump-table handlers only checked for a negative count, then did `vec![Null; count]`. `PUSHINT32 0x7fffffff; NEWARRAY` caused capacity overflow/OOM. C# rejects counts above `Limits.MaxStackSize` (2,048). The fast interpreter already capped, so the two engines disagreed.  
**Fix:** `bounded_collection_count` in `collections.rs:107` rejects counts above 2,048 before allocation; all three constructors use it. Tests cover the boundary and `i32::MAX`.  
**Evidence:** `neo-vm/src/semantics/collections.rs:107-124, 490-500`.

### C4. Built-in syscall table overrode the authoritative call flags
**Where:** `neo-vm/src/interop_service.rs`, `neo-core/src/smart_contract/application_engine/interop_host.rs`  
**Problem:** `invoke_by_hash` checked the **static** syscall registry’s `call_flags` before dispatching to the host. That registry approximated flags with string-prefix rules: all `System.Storage.*` were marked READ|WRITE, `System.Contract.CreateStandardAccount` was marked READ_STATES, etc. The real per-engine registrations use exact C# flags. Consequences: `CreateStandardAccount`/`CreateMultisigAccount` failed in READ_STATES contexts, storage reads required write permission, and three test suites failed.  
**Fix:** The built-in fast path now only identifies a host syscall and dispatches; the authoritative `ApplicationEngine::interop_handlers` entry checks flags and charges gas. Added the missing flag check in `InteropHost::invoke_syscall`. This fixed `application_engine_contract_tests`, `crypto_lib_verify_with_ecdsa_custom_tx_witness_*`, and the oracle response-fee test.  
**Evidence:** `neo-vm/src/interop_service.rs:213`, `neo-core/src/smart_contract/application_engine/interop_host.rs:3`.

### C5. Opcode price table diverged from C# (consensus gas)
**Where:** `neo-core/src/smart_contract/application_engine/op_code_prices.rs`  
**Problem:** `NEWSTRUCT0` (0xC5) used the `NEWARRAY` price (512 instead of 16), `NEWMAP` (0xC8) used the `APPEND` price (8,192 instead of 8), and `ASSERTMSG` (0xE1) was later found to be 0 instead of 1. Gas consumption feeds GAS balances and state root, so this is consensus-critical.  
**Fix:** Rewrote the table from the C# v3.10.1 dictionary and added an exhaustive 256-entry test transcribed from C#. Also added a test comment warning that opcode numbering is easy to get wrong.  
**Evidence:** `neo-core/.../op_code_prices.rs:15,69`.

### C6. `ChangeView` wire format omitted `RejectedHashes`
**Where:** `neo-consensus/src/messages/change_view.rs`  
**Problem:** Neo 3.10.1 `ChangeView` serializes a varint-prefixed `UInt256[]` when `Reason` is `TxInvalid` or `TxRejectedByPolicy`. Rust serialized only timestamp+reason, so C# peers got EOF while reading the array and rejected the message; Rust also discarded C# hashes.  
**Fix:** Added `rejected_hashes`, conditional serialization/deserialization, `with_rejected_hashes`, and a wire-format test. Updated the stale refinement test that expected 9 bytes.  
**Evidence:** `neo-consensus/src/messages/change_view.rs:27,75,121`.

### C7. Proposal timestamp used local wall clock, not the parent header
**Where:** `neo-consensus/src/service/proposal.rs`, `neo-node/src/consensus.rs`  
**Problem:** On persist, the adapter passed `current_time_ms()` as `prev_timestamp`; proposal creation used `now`. C# uses `max(now, PrevHeader.Timestamp + 1)` and validates against the parent header timestamp. On a healthy network a proposal is often built before a node finishes persisting the parent, so honest proposals were rejected as `timestamp <= prev_timestamp`.  
**Fix:** `proposal.rs:50` clamps to `max(now, prev_timestamp + 1)`; `neo-node/src/consensus.rs:523` passes the persisted header’s timestamp.  
**Evidence:** `neo-consensus/src/service/proposal.rs:50`, `neo-node/src/consensus.rs:523`.

### C8. TaskManager retained unverified block payloads without a budget
**Where:** `neo-core/src/network/p2p/task_manager/completion_flow.rs`, `task_session.rs`  
**Problem:** Every delivered `Block` was stored whole in `session.received_block` before validation, with no byte/count limit and no check that the block was requested. With fast-sync’s 10,000-block window and a 32 MiB P2P payload cap, one peer could pin hundreds of GB.  
**Fix:** Only blocks that match an in-flight index/inventory task (or an already-cached candidate) are stored; per-peer budgets are 256 blocks / 64 MiB (`TaskSession::MAX_RECEIVED_BLOCKS`, `MAX_RECEIVED_BLOCK_BYTES`), with byte accounting on store/remove/retention. Per-peer task preallocation dropped from ~6 MB to a small map, and `MAX_PENDING_TASKS` dropped from 50,000 to 4,096.  
**Evidence:** `task_session.rs:52,133`, `completion_flow.rs:55`; test `received_block_budget_is_enforced_and_released`.

### C9. Per-height candidate cap could censor an honest block
**Where:** `neo-core/src/ledger/blockchain/block_processing.rs`  
**Problem:** When the 3-candidate cap was full, the newcomer was silently dropped. An attacker could pre-fill future heights with cheap bogus blocks; the honest block’s hash was already in the peer dedup caches, so re-delivery was suppressed and the chain stalled.  
**Fix:** The cache now evicts the oldest candidate and accepts the newcomer (memory still bounded); verification failures try remaining candidates instead of stopping the sequence.  
**Evidence:** `block_processing.rs:147-230, 287-293, 344-350`.

### C10. Recovery messages could poison `preparation_hash`
**Where:** `neo-consensus/src/service/handlers/recovery.rs`  
**Problem:** A single validator’s unsolicited `RecoveryMessage` could set `context.preparation_hash` before the primary’s `PrepareRequest` arrived. Every honest `PrepareResponse` then failed `HashMismatch`, preventing quorum. C# keeps the recovery hash local.  
**Fix:** The recovery hash is used only for reconstructing recovery responses; it no longer overwrites `context.preparation_hash`.  
**Evidence:** `neo-consensus/src/service/handlers/recovery.rs:210`.

### H1–H6. VM jump-table divergences
All in `neo-vm`, all against Neo.VM 3.10.1:

| # | Fix | Location |
|---|---|---|
| H1 | `EQUAL`/`NOTEQUAL`: `Buffer`/`Array`/`Map` now compare by reference; `Struct` compares structurally with `MaxStackSize`/`MaxComparableSize` budgets; `ByteString` enforces the comparable-size budget | `stack_item/stack_item.rs:694` |
| H2 | `LT`/`LE`/`GT`/`GE`: any `Null` operand returns `false` (previously `LT(null,x)=true`, `LE(null,null)=true`, ...) | `jump_table/numeric.rs:269` |
| H3 | `VALUES` accepts `Array`/`Struct` and clones nested `Struct`s with limits (previously only `Map`) | `jump_table/compound.rs:435` |
| H4 | `POW`: exponent must be in `0..=256` (C# `AssertShift`), no “estimated bits” shortcut that accepted `POW(1,1000)` | `semantics/arithmetic.rs:154` |
| H5 | Implicit end-of-script `RET` now enforces `RVCount` instead of clamping | `execution_engine/execution.rs:65` |
| H6 | `PUSHDATA*` asserts `MaxItemSize`; `PUSHA` uses checked address arithmetic | `jump_table/push.rs:120,148` |

### Additional fixes
- **MPT root parity:** added a Rust test replaying C# `UT_Trie.TestTryDelete`; the root hash matches the C# expected value `0xcb0692…3408`. This is the first independent C# root vector in the tree (`neo-crypto/src/mpt_trie/tests.rs:1560`).
- **P2P height updates:** `Pong` and `Headers` now forward peer heights to TaskManager (C# does both); previously a peer that never sent `PING` stayed frozen at its handshake height and sync could stall (`peer_messages.rs:39,89`).
- **Message-copy path:** inbound messages are serialized/decompressed only when a plugin handler is registered; previously every frame paid for a second LZ4 decode and large transient copies (`remote_node.rs:104`).
- **Test-suite correctness:** fixed stale/wrong expectations in `native_token_tests`, `opcode_price_refinement_tests`, `opcode_full_256_refinement_tests`, `financial_overflow_checks`, `change_view_refinement_tests`, `neo-tee` timestamp test, and MPT proptests that generated duplicate keys; gated two runtime-only `neo-core` test targets.

### C11-C36. Production-readiness fixes (second pass)

| # | Fix | Location |
|---|---|---|
| C11 | dBFT now requires all proposed transactions before signing a Commit or emitting `BlockCommitted`; `transactions_available` is transient and reset on restart/view change, and is set only by `on_transactions_received` after the upper layer validates the full set | `neo-consensus/src/context/mod.rs`, `service/proposal.rs`, `handlers/commit.rs` |
| C12 | Current-view commits received before the proposal are buffered by validator, witness-checked, and re-verified against the block hash when `PrepareRequest` installs it; pending commits are cleared on a new view/block | `service/handlers/commit.rs`, `service/core.rs`, `handlers/prepare.rs` |
| C13 | `ChangeView.RejectedHashes` is retained per validator (C# `InvalidTransactions` set), >F rejections are excluded from proposal selection, and this node broadcasts its own rejected hashes in `ChangeView`; adopting a higher view re-broadcasts `ChangeAgreement` | `context/mod.rs`, `service/handlers/change_view.rs`, `neo-node/src/consensus.rs` |
| C14 | Pre-Gorgon `HASKEY` no longer applies the post-Gorgon `MaxItemSize` bound; arrays/structs and byte strings use the correct fork rule. `ENDTRY` inside `FINALLY` now faults; `ENDFINALLY` no longer requires `Finally` state (C# `JumpTable.Control.cs`) | `neo-vm/src/jump_table/compound.rs`, `semantics/collections.rs`, `execution_engine/exception.rs`, `vm/limits.rs` |
| C15 | All 121 architecture-conformance tests now pass: `StackItem` byte/boolean/CONVERT paths delegate to `StackValue`/`convert_value`, the ordered-dictionary facade is gone, and the VM imports shared scalars directly | `neo-vm/src/stack_item/stack_item.rs`, `neo-tests/tests/no_local_neo_vm_dependency.rs` |
| C16 | Library targets are warning-free: removed unused imports/constants, gated test-only helper, documented the unwired tiered-cache prototype, formatted modified files, and reconciled `metadata.msrv` with `rust-version = 1.88` | workspace |
| C17 | Fast-interpreter `EQUAL`/`NOTEQUAL`/`JMPEQ`/`JMPNE` structural comparison now uses the same bounded budget as `StackItem::equals_with_limits` (2,048 struct items / 65,536 comparable bytes) instead of unbounded recursion; regression tests cover the budget boundary and oversized ByteString | `neo-vm/src/interpreter/helpers/values.rs`, `executor/numeric_ops.rs`, `executor/mod.rs` |
| C18 | Per-transaction native-contract seeding now clones `Arc<ContractState>` handles instead of deep-cloning NEF scripts/manifests. Benchmarked **13.27 µs → 105.78 ns** per seed (~125×); a 1,000-tx block avoids ~13.17 ms of copying | `application_engine/state.rs`, `neo_system/persistence.rs`, `benches-package/benches/native_contracts.rs`, [PERFORMANCE-2026-10-17.md](PERFORMANCE-2026-10-17.md) |
| C19 | The 10 stateless native contracts are now shared from a process-wide `OnceLock`; only stateful `ContractManagement` is built per engine. `NativeRegistry::new` **13.30 µs → 1.45 µs**; preloaded engine init **46.31 µs → 29.59 µs** (≈16.3 µs production path after shared state seeding) | `smart_contract/native/mod.rs`, [PERFORMANCE-2026-10-17.md](PERFORMANCE-2026-10-17.md) |
| C20 | `CheckWitness` now decodes and validates 33-byte secp256r1 public keys before hashing the redeem script, matching C# `ECPoint.DecodePoint` FAULT behavior instead of silently returning `false`; tests cover invalid encodings and a valid P-256 generator | `application_engine/helper.rs`, `runtime.rs`, `external_vm.rs`, `neo-core/tests/check_witness_pubkey_tests.rs` |
| C21 | `MemoryStore` snapshots share an immutable `Arc<BTreeMap>` and prefix `find` uses `BTreeMap` ranges instead of cloning/scanning the whole store. Empty-block persistence stabilized at **1.02 ms** and one-RET-tx at **1.32 ms** (previously drifted to ~4.0/~5.2 ms as the chain grew); tests cover prefix order and snapshot isolation | `neo-storage/src/persistence/providers/memory_store.rs`, `memory_snapshot.rs`, `benches-package/benches/block_persistence.rs` |
| C22 | `LedgerTransactionStates::mark_vm_state` now uses a lazily built hash → index map instead of scanning the block's whole transaction-state list for every transaction (O(N²) → O(N)). Measured: one `RET` tx **1.32 ms**, ten `RET` txs **1.40 ms** | `neo-core/src/smart_contract/native/ledger_contract/state.rs`, `benches-package/benches/block_persistence.rs` |
| C23 | Windows production RocksDB build fixed: `.cargo/config.toml` sets `_WIN32_WINNT=0x0A00` so MinGW exposes `FILE_ID_INFO`/`FileIdInfo`; `cargo check -p neo-core --features rocksdb` and `neo-node --features full` now pass | `.cargo/config.toml` |
| C24 | NeoFS gRPC build fixed: `tonic`/`tonic-build` pinned to **0.13** (matching generated code and `prost` 0.13) and the build script uses `compile_protos`; `neo-core --features neofs-grpc` and `neo-node --features full` now compile | `neo-core/Cargo.toml`, `neo-core/build.rs` |
| C25 | Added a real C# Neo.VM 3.10.1 differential vector test: the C# reference `ExecutionEngine` produced state/stack outputs for arithmetic, comparison, bitwise, POW, MOD-by-zero, PUSHDATA, CAT, SUBSTR, LEFT, RIGHT, SIZE, and CALL/RET scripts; the Rust `interpret` path asserts the same outputs | `neo-vm/tests/csharp_vm_vectors.rs` |
| C26 | Fixed `MaxStackSize` false positives: the reference counter over-counts across nested `RET` frames, so enforcement now measures the actual live evaluation-stack slots across invocation/result stacks while still bounding memory. **All 14 real MainNet block replay tests now pass** (blocks 4410 … 1,465,790), including C# state-root assertions; the previously failing 1,268,131 tx HALTed with the exact C# gas **40,436,620** | `neo-vm/src/execution_engine/execution.rs`, `neo-core/tests/mainnet_block_*_repro.rs` |
| C27 | Bulk C# state-root differential: compared **1,460,685** stored MainNet roots up to height 1,465,790 against `data/reference_stateroots.jsonl`; **0 mismatches**; 5,106 heights absent from the local backup are skipped explicitly | `neo-core/tests/mainnet_state_roots_vs_csharp.rs`, `data/Plugins/StateService/StateRoot` |
| C28 | `RET` now mirrors C# `CopyTo` for the value transfer and then releases the callee source-stack references **after** context unload, so returned values are counted once. Native iterator test and all 14 MainNet replay fixtures pass | `neo-vm/src/jump_table/control.rs`, `neo-vm/src/evaluation_stack.rs` |
| C29 | Contiguous full stored-chain integrity check: every block **0–1,352,010** (1,352,011 blocks, 609,487 transactions) was loaded from the MainNet RocksDB, re-hashed, checked against its previous hash, and Merkle-root validated. All passed in **27.3 s** | `neo-core/tests/mainnet_chain_integrity.rs`, `data/mainnet-protocol-consistency` |
| C30 | Added a self-hosted MainNet validation workflow that runs the contiguous chain check, all 14 replay fixtures, and the bulk C# state-root differential against local multi-GB backups | `.github/workflows/mainnet-validation.yml` |
| C31 | Extended the 4-validator dBFT harness with an offline-validator mode; with n=4, f=1, the three online validators still commit a block | `neo-consensus/src/service/tests/{helpers,persist}.rs` |
| C32 | Locked C# `NEWARRAY` reference-count parity: 1,000 null children + the array = **1,001**, matching the C# reference | `neo-vm/tests/csharp_vm_vectors.rs` |
| C33 | Achieved exact reference-counter enforcement: children are added/released on first/last stack reference, runtime append/set only adjusts references while the compound is stack-referenced, and decrements saturate instead of wrapping. `MaxStackSize` now uses the real counter with **no slot-based fallback**; the 1,268,131 transaction HALTs with the exact C# gas, and all 14 MainNet replays pass | `neo-vm/src/reference_counter.rs`, `neo-vm/src/stack_item/{array,struct_item,map}.rs`, `neo-vm/src/execution_engine/execution.rs` |
| C34 | Full contiguous execution replay verified: **blocks 200,000..=1,352,000 (1,152,001 roots)** matched the C# reference with **0 gaps and 0 mismatches**, in chunked release runs. Two replay-harness limitations were fixed: Ledger storage is read from the chain store, and `Ledger.currentIndex` is overridden with the historical height (`block.index-1`) instead of the live chain tip. The latter explains all eight earlier `IdoContract` divergences (blocks 885,963–983,394); they were harness artifacts, not node bugs. An earlier from-genesis run matched 99,877 blocks | `neo-core/tests/mainnet_contiguous_replay.rs` |
| C35 | Added real multi-node dBFT network tests: four `ConsensusService` instances exchange signed PrepareRequest/PrepareResponse/Commit payloads over **UDP** and, in a second test, over **TCP**, committing block 1 in both cases. These are real transport tests, not the direct in-process harness | `neo-consensus/src/service/tests/udp_cluster.rs`, `tcp_cluster.rs` |
| C36 | Fixed two hot-path native/storage lookups. `ContractManagement.getContract` hydrated every contract record on each call (~4.5 ms); it now reads the requested snapshot key directly (~1.5 µs), as does `getContractById`. `ApplicationEngine::get_storage_context` also deserialized the full contract state on every `Storage.GetContext`; it now reuses the engine contract cache. Heavy MainNet replay (blocks 1,346,000..=1,352,000) dropped from **286 s to 158.5 s** with identical roots; the storage-context change alone was 175.4 s to 158.5 s | `neo-core/src/smart_contract/native/contract_management/{native_impl,query}.rs`, `application_engine/load_execute_storage.rs` |

## 4. Residual validation work

These items cannot be closed by unit tests in this repository; they are the remaining gates before calling the node mainnet-certified.

1. **Full C# differential replay and state-root comparison.** One C# MPT deletion vector passes and the native-contract metadata is checked, but full-chain state-root equivalence against a C# v3.10.1 node is not in CI. Wire `tools/state-root-diff` into a replay job over sampled MainNet ranges and publish mismatches. This is the single highest-value remaining validation.

2. **Real multi-node dBFT network test.** The 4-validator in-process dBFT harness now passes block-commit and multi-round tests (`service::tests::persist`), and the transaction-availability, pending-commit, invalid-tx, and ChangeAgreement paths are unit-tested. A real Rust + C# validator cluster over TCP, through view changes, recovery, restarts, and network partitions, is still needed to prove liveness and no double-commit.

3. **Native-contract execution vectors.** IDs, hashes, manifests, fees, and hardfork windows are checked; add C#-captured execution vectors for NEO governance (candidate/committee/bonus), ContractManagement lifecycle, Oracle/Notary, and Ledger serialization.

4. **Pre-Echidna `SUBSTR`.** Pre-Gorgon `HASKEY` and shift behavior are now fork-selectable. The pre-Echidna `SUBSTR` difference is an overflow edge that FAULTs in both implementations; it should still be covered by a historical replay test before claiming full pre-Echidna parity.

5. **Two VM execution paths.** The fast interpreter and jump table now share conversion/truthiness rules and the same bounded structural-equality budget, but a differential harness over both engines (including dead `System.Contract.Call` scripts) is still required to prove they cannot diverge.

6. **Unsafe/`unwrap` audit.** 85 `unsafe` blocks in `neo-core` and 76 in `neo-vm` were not line-by-line audited; attacker-reachable `unwrap`/`expect` remains a remote-DoS risk. Run Miri/fuzz targets and an explicit audit before mainnet.

7. **RPC response-shape diff.** 57+ methods are registered; exact JSON shape/error-code parity with C# RPC has not been exhaustively diffed.

8. **Contiguous execution replay is verified from block 200,000 through 1,352,000.** The bulk differential compared **1,460,685** stored MainNet roots through height 1,465,790 with **0 mismatches**; the stored chain check re-hashed and Merkle-validated **1,352,011 contiguous blocks / 609,487 transactions** with 0 failures. The new from-genesis replay executes every transaction plus OnPersist/PostPersist against the previous state trie and matched **99,877 blocks** before hitting a gap in the C# reference JSONL at 99,878. All 14 checked-in block replay fixtures pass with C# gas/state-root assertions; block 1,268,131 halts with the exact C# gas `40,436,620`. The exact reference counter enforces `MaxStackSize` directly. The earlier block 238,711 GAS-supply mismatch was a replay-harness limitation: Ledger-contract storage is excluded from the StateService trie, so `Ledger.getBlock` returned Null. Routing Ledger keys to the chain store and overriding `Ledger.currentIndex` with the historical height makes all **1,152,001 roots** from 200,000 through 1,352,000 match with 0 gaps and 0 mismatches. A four-node TCP and a four-node UDP dBFT cluster also commit block 1 over real sockets. A four-node dBFT cluster also commits block 1 over real UDP sockets.

---

## 5. Completeness and compatibility assessment

| Area | Status | Evidence / gaps |
|---|---|---|
| Block/transaction serialization | **Strong** | Genesis hash, 27 real MainNet blocks, byte round-trip, merkle/duplicate checks at deserialize |
| VM opcodes | **Broad; shared semantics** | 177 VM unit tests pass; pre-Gorgon HASKEY/TRY and bounded EQUAL fixed; differential harness still recommended (§4.5) |
| Opcode gas | **Fixed to C#** | Exhaustive 256-entry table test; `NEWSTRUCT0`/`NEWMAP`/`ASSERTMSG` corrected |
| Native contracts | **Broad, partial execution proof** | IDs/hashes/manifests/fees checked; governance/lifecycle bodies need C# vectors (§4.3) |
| dBFT | **Implemented, safety gates added** | Wire format/signing domains checked; tx-availability, pending commits, invalid-tx/ChangeAgreement implemented; cluster test pending (§4.2) |
| P2P | **Implemented, unproven at scale** | Framing/compression/handshake checks pass; no full MainNet sync run; TaskManager hardening added |
| JSON-RPC | **Broad** | 57+ methods registered; exact C# response-shape diff pending (§4.7) |
| State service / MPT | **Implemented; C# vector passes** | C# `TestTryDelete` root matches; full-chain equivalence pending (§4.1) |
| Storage / RocksDB | **Implemented** | Crash consistency, batch/WAL tuning not independently verified |
| Performance claims | **Not substantiated** | See §6 |

---

## 6. Architecture, performance, and Rust practice

### Performance claims vs. reality
- **Parallel execution is not in the production path.** `optimistic_executor`, `parallel_execution_integration`, and `conflict_detector` are `runtime`-gated and test-only; `NeoSystem::persist_block` executes transactions serially.
- **Prefetch/async state root/tiered storage are not wired.** `prefetch_*` and `pipeline_integration_example` are behind a non-default `prefetch` feature; `tiered_storage` and `async_state_root` compile but are not used by the block pipeline.
- **No evidence for the “10×–50×” or “100×” claims.** The original claims still have no end-to-end benchmark; there are now two measured component optimizations (native-contract seeding ~125×, native registry construction ~9×) plus the existing VM/serialization microbenchmarks. See [PERFORMANCE-2026-10-17.md](PERFORMANCE-2026-10-17.md).
- **Measured win 1:** per-transaction native-contract seeding dropped from **13.27 µs** (deep clone) to **105.78 ns** (Arc clone), saving roughly **13.17 ms per 1,000-tx block** on that step alone.
- **Measured win 2:** `NativeRegistry::new` dropped from **13.30 µs** to **1.45 µs** by sharing the 10 stateless native contracts; preloaded engine construction dropped from **46.31 µs** to **29.59 µs**, and the production shared-state path is ≈**16.3 µs** per transaction.
- **Measured state-root cost:** a 1,000-key MPT build+commit is **11.61 ms**; 10,000 keys is **154.33 ms**; 1,000 lookups are **99.09 µs**. This is now the largest measured per-block state cost and the next structural optimization target (lazy path hashing).
- **Measured P2P framing:** a 500-transaction compressed block encodes in **28.30 µs** and full-decodes in **404.73 µs**; protocol decode is **402.04 µs** (per-transaction deserialization/validation + hashing + merkle rebuild), while framing/LZ4 deserialization is only **1.98 µs**.
- **Measured witness cost:** secp256r1 verify is **141.26 µs** per 32-byte message; a two-witness transaction pays ~**280 µs** before VM execution. Witness checks are independent and a candidate for bounded parallelism, but only with a consensus-order/C# differential proof.
- **Measured end-to-end block pipeline:** persisting an empty block through `NeoSystem::persist_block` (native `OnPersist`, ledger, write cache, commit) costs **1.02 ms** on an in-memory testnet system; one RET-transaction block costs **1.32 ms**. With the durable RocksDB provider the empty block is **1.12 ms**. This is the fixed per-block overhead before real transaction workloads and state-root work.
- **Actual hot-path costs that remain:** per-transaction `ApplicationEngine` construction and interop registration, script copying, signature verification, RocksDB commit, and state-root maintenance. `find_block_in_cache_by_index` is still a linear scan, though it is only a fallback path.
- **Fixed in this review:** a second LZ4 decode/large copy per inbound P2P frame when no plugin handler is registered (`remote_node.rs:104`).

### Architecture
- Layering is mostly sensible (foundation → VM → core → services), but `neo-core` is a **126k-line monolith** (662 files) containing ledger, P2P, RPC helpers, native contracts, state service, wallets, oracle, and TEE-adjacent code.
- There are **two P2P stacks** (`neo-p2p` type-only crate vs `neo-core::network::p2p`), legacy `connection.rs`/`peer.rs` files, and duplicate `message.rs`/`messages.rs` concepts. This increases maintenance risk and makes “which path is live?” non-obvious.
- Feature flags are brittle: two `neo-core` integration tests only compiled when another workspace member unified `runtime`; they are now gated with `#![cfg(feature = "runtime")]`. Other feature-only modules still compile into default builds.
- `rust-version = "1.88"` and `metadata.msrv` now agree; CI should still build with the declared MSRV, not only the developer toolchain.
- **Production feature build is now verified on Windows/MinGW:** `neo-node --features full` compiles after fixing the RocksDB `_WIN32_WINNT` define and aligning `tonic`/`tonic-build` at 0.13 with `compile_protos`.
- `panic = "abort"` in release plus ~1,300 `unwrap()`/2,400 `expect()` occurrences in `neo-core` is a remote-DoS risk wherever attacker-controlled data reaches them. The block-path critical ones found here were fixed, but a systematic `unwrap` audit is still needed.
- Unsafe code: 85 blocks in `neo-core`, 76 in `neo-vm`; the hot ones are arena/pool and evaluation-stack fast paths. They were not line-by-line audited in this review.

### Test hygiene
- The suite is large (**4,517 passing**) and includes useful C# parity vectors. Several stale expectations were fixed during this review (opcode prices, `ChangeView` length, empty-value MPT proptests, TEE timing).
- `cargo check --workspace` is now **warning-free for library targets**; a few test files still have unused-import/naming warnings. The unwired tiered-cache prototype is explicitly marked and documented rather than emitting missing-doc/dead-code noise.
- A real CI gate should include: `cargo fmt --check`, `cargo clippy -- -D warnings` (or a ratcheted allowlist), `cargo test --workspace`, and a C# differential job.

---

## 7. Prioritized remediation plan

**P0 — before any validator/mainnet use (validation, not known code defects)**
1. Add a C# interop harness: run a C# v3.10.1 node/VM against this node for (a) VM script vectors, (b) full block replay, (c) state roots at hardfork heights, (d) dBFT message capture.
2. Run a multi-node dBFT cluster (Rust + C# validators) through view changes, recovery, and restarts; verify liveness and no double-commit.
3. Wire `tools/state-root-diff` into CI over a sampled MainNet range and publish mismatches.
4. Add native-contract execution vectors for NEO governance, ContractManagement, Oracle/Notary, and Ledger.

**P1 — hardening**
5. Differential-harness the jump table vs fast interpreter, including dead `System.Contract.Call` scripts.
6. Audit attacker-reachable `unwrap`/`expect` and the 85/76 `unsafe` blocks; run Miri/fuzz targets.
7. Add a pre-Echidna historical replay vector for `SUBSTR`.
8. Exact JSON-RPC response/error-shape diff against C#.

**P2 — performance and architecture**
9. Either wire the parallel/prefetch/tiered-storage modules into the real persist path with correctness proofs, or delete/mark them as prototypes so the documentation stops overstating readiness.
10. Add benchmarks for block persist, VM execution, signature verification, and state-root calculation, with CI thresholds.
11. Consolidate the duplicate P2P/message stacks; remove legacy dead modules.

---

## 8. Verification summary

| Check | Result |
|---|---|
| `cargo check --workspace` | Pass, **0 library warnings** (some legacy test-file style warnings remain) |
| `cargo check -p neo-core --features rocksdb` | **Pass** (Windows/MinGW production storage) |
| `cargo check -p neo-node --features full` | **Pass** (RocksDB + gRPC + monitoring + UPnP) |
| `cargo test --workspace --no-fail-fast` | **4,517 passed, 0 failed** (220 suites) |
| `neo-tests --test no_local_neo_vm_dependency` | **121 passed, 0 failed** |
| `cargo test -p neo-core --lib` | 633 passed |
| `cargo test -p neo-consensus` | 116 unit + all integration tests passed |
| C# MPT `TestTryDelete` root | Pass (`0xcb0692…3408`) |
| Opcode price table vs C# | Pass (all 256 entries, exhaustive test) |
| C# block vectors (27 MainNet blocks) | Pass (existing suite) |
| Genesis hash/next-consensus vs C# | Pass (existing suite) |
| Native-contract seeding benchmark | 13.27 µs → 105.78 ns (Arc sharing, ~125×) |
| Native registry / engine init benchmark | registry 13.30 µs → 1.45 µs; preloaded engine 46.31 µs → 29.59 µs (~16.3 µs production path) |
| MPT state-root benchmark | 1,000 puts + commit: 11.61 ms; 10,000: 154.33 ms; 1,000 gets: 99.09 µs |
| P2P frame benchmark | 500-tx compressed block: encode 28.30 µs; full decode 404.73 µs (protocol decode 402.04 µs; framing/LZ4 1.98 µs) |
| Signature benchmark | secp256r1 verify 141.26 µs per witness (32-byte message) |
| C# Neo.VM differential vectors | **26/26 pass** (`cargo test -p neo-vm --test csharp_vm_vectors`) |
| MainNet block replay | **14/14 pass** against the local state-root DB, including C# state-root assertions |
| From-genesis execution replay | **99,877 blocks**, 0 state-root mismatches (stops at reference-data gap 99,878) |
| Stored MainNet chain | **1,352,011 blocks / 609,487 txs**, contiguous hashes + Merkle roots, 0 failures |
| Bulk C# state-root diff | **1,460,685 roots compared, 0 mismatches** (heights 0–1,465,790) |
| dBFT 4-validator harness | **4/4 pass**: round start, block commit, multiple rounds, one validator offline |
| dBFT UDP network cluster | **4/4 nodes commit block 1 over real UDP sockets** |
| dBFT TCP network cluster | **4/4 nodes commit block 1 over real TCP sockets** |
| `RET` C# `CopyTo` semantics | native iterator test + 14/14 MainNet replays pass |
| End-to-end empty-block persistence | 1.02 ms per block; 1 RET-tx 1.32 ms; 10 RET-tx 1.40 ms (native OnPersist + ledger + commit) |
| RocksDB production persistence | Empty block **1.12 ms** (WAL + write batch) |

**Bottom line:** the code-level blockers found in the review are fixed: unverified-block persistence, VM stack/allocation limits, syscall flags, opcode gas, dBFT wire format/timestamp, dBFT transaction availability and pending commits, invalid-transaction tracking, pre-Gorgon `HASKEY`, `TRY`/`ENDTRY` state, bounded interpreter equality, 33-byte `CheckWitness` validation, P2P memory bounds, architecture-conformance tests, and the production `full` feature build (RocksDB + gRPC). C# and chain evidence is now substantial: 26 Neo.VM vectors pass, all 14 checked-in MainNet block replay fixtures pass, **1,460,685 MainNet state roots through height 1,465,790 match the C# reference with 0 mismatches**, and the stored chain’s **1,352,011 contiguous blocks / 609,487 transactions** pass hash, previous-hash, and Merkle-root validation. A from-genesis execution replay matched **99,877 consecutive blocks**, `RET` uses C# `CopyTo` semantics, and the 4-validator in-process dBFT harness commits blocks across multiple rounds and with one validator offline, and four-node dBFT clusters commit block 1 over real UDP and TCP sockets. Native-contract seeding, native-registry construction, memory-store snapshots, and per-transaction ledger-state lookup are measured and substantially faster. Library targets build warning-free and all 4,517 default tests pass. The exact reference counter now enforces `MaxStackSize`; the remaining gates are execution replay beyond the verified range and a real TCP deployment. Until those are published, the honest label is **testnet/private-network ready**, not “mainnet-certified.”
