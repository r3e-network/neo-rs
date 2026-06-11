# neo-rs Refactor & Parity Roadmap

> **STATUS 2026-05-30 (~29 commits, HEAD 0bd65dda): Phase 3 protocol parity COMPLETE & VERIFIED GREEN**
> across all suites (`cargo test --workspace` + `neo-rpc --features server` 577 + `neo-tests` 120 + `neo-vm-rs`), 0 failures.
> Remaining: **B3/B4/B5** (neo-vm-rs limit alignment, multi-platform — see plan below) and **Phase 4** crate-boundary
> refactor (G–P, incl N1 P2P move). Both are large, focused efforts. Verify with the feature-gated suites, not just `--workspace`.


> Prioritized execution plan from the 2026-05-29 audit. Driving requirement: **100% protocol
> compatibility with C# Neo v3.9.1/2**. Order: consensus-breaking → interop-breaking →
> architecture/crate-boundary → idiomatic-Rust/ecosystem. Every increment ends GREEN
> (`cargo check --workspace --all-targets`) and is committed separately.
> Full evidence in `audit-findings.md`. Each item: VERIFY against C# source before editing.

## Legend
`[ ]` todo · `[~]` in progress · `[x]` done · effort S/M/L · ⚠ removes a Rust-added safety check (verify C# truly omits it)

---

## PHASE 3 — Protocol parity (MUST fix for 100% compatibility)

### Batch A — Native contracts & hardforks (neo-core)
- [x] A1 (S) Removed fabricated HF_Faun heights; Faun/Gorgon unscheduled (match C# config). HF_Gorgon enum ordinal verified == C#. **Note: the HF_Faun *feature set* (Treasury, Policy pico-fee/whitelist/recoverFund, StdLib hex, Local storage) is LEGITIMATE forward-work matching C# v3.9 dev — kept, now dormant.** Commit a179959b.
- [x] A2 (M) Removed fabricated TokenManagement native contract (id -12). Commit 3ad4e0df.
- [x] A3 (S) base64Url gate HfCockatrice → HfEchidna. Commit a179959b.
- [x] A4 (S) Removed over-enforced StdLib MaxLength (memorySearch.value, stringSplit.separator, jsonDeserialize). Commit a179959b.
- [x] A5 (M) Oracle userData → 512 serialized bytes. The native dispatcher already BinarySerializer-serializes `Any` args, so args[3] IS the serialized form; only the limit was wrong. Commit e2c642c9.

### Batch B — NeoVM limits & engine unification (neo-vm-rs ⚠ sibling repo + neo-core/neo_vm)
- [x] B1 (S) MaxItemSize → 131070. neo-vm-rs commit b0913eb.
- [x] B2 (S) MaxComparableSize → 65536. neo-vm-rs commit b0913eb.
**B3/B4/B5 PLAN (user decision: align neo-vm-rs to C#, keep zk/riscv working). Investigated 2026-05-30 — neo-vm-rs hardcodes limits as free consts, NOT the ExecutionEngineLimits struct field (which is unused by the interpreter/semantics).**
- [ ] B3 (M) MAX_ITEM_SIZE: `neo-vm-rs/src/vm/limits.rs:15` free const = 1MB, used in `semantics/collections.rs:98` (new_buffer) + `semantics/splice.rs:14` (cat_values). Thread `&ExecutionEngineLimits` into these fns + callers (`executor/compound_ops.rs`, `executor/mod.rs`, `interpreter/api.rs` add `_with_limits` variants); consensus default 131070. EASY, low zk risk (131070<1MB is more restrictive). neo-core `external_vm.rs` must pass the engine limits.
- [ ] B4 (M) MAX_CALL_DEPTH=64 is a **fixed-capacity `[MaybeUninit<CallFrame>;64]` array** (`state.rs:110`, `state/call_stack.rs:14`) deliberately sized for PolkaVM bump-allocator safety (`#[cfg(target_arch="riscv32")]`). Do NOT naively raise. Feature-gate: riscv32 keeps 64; non-riscv32 (consensus) uses 1024 array (or Vec). HARD — risk to PolkaVM if made dynamic.
- [ ] B5 (L) No ReferenceCounter in neo-vm-rs (eval-stack-length only; compound = `Vec<StackValue>`, no Rc — `executor/mod.rs:145`). C# counts eval+slots+nested per instruction. Implement an **opt-in** reference counter (flag in ExecutionEngineLimits or feature): ON for Neo consensus, OFF for zk (per-instruction counting is too expensive for proofs). Deepest change; design carefully. Also MAX_STACK_SIZE check should use `limits.max_stack_size` not the const (`compound_ops.rs:150,163,180`).
- Phase order: B3 → B4(feature-gate) → B5(opt-in refcounter) → verify neo-riscv-vm + zkvm still build/test. Backward-compatible: keep existing entry points defaulting to DEFAULT limits; add `_with_limits` variants.
- [x] B6 (L) MaxComparableSize enforcement + C# reference equality in EQUAL/NOTEQUAL. Faithful port of C# Equals(limits): ByteString size-budget, Struct count/comparable budgets, Array/Map→reference equality (verified vs C# EQUAL.json vectors), primitive type-strictness preserved. Commit c9512098. (Agent-drafted, reviewed against C# source by me.)
- [x] B7 (S) MaxStackSize `>=` → `>`. Commit 51755d79.
- [x] B8 (S) ENDTRY-in-FINALLY faults. Commit 51755d79.

### Batch C — Cryptography (neo-crypto + neo-core/wallets)
- [x] C1 (S) NamedCurveHash Keccak bytes → 122/123. Commit 3214490a.
- [x] C2 (S) secp256k1 high-s acceptance (normalize_s) + regression test. Commit 3214490a.
- [x] C3 (S) NEP-2 AES-256-ECB **and** Base58Check (found extra base64 encoding bug). Commit ab6f3637.

### Batch D — Ledger / serialization (neo-core)
- [x] D1 (S) Removed fabricated 2MB MAX_BLOCK_SIZE gate. Commit 59bd7805.
- [x] D2 (S) Header verify → primary-index + continuity + witness only (dropped future-ts + witness-size/0xFF). Commit 59bd7805.
- [x] D3 (M) Block::verify is header-only (C# parity); removed per-tx re-verify from import. Commit 59bd7805.
- [x] D4 (S) Block deserialize enforces merkle-root + no-duplicate-tx (C# DeserializeTransactions). Commit 59bd7805. Guard test updated (0df26aa2).
- [x] D5 (S) Removed MaxTransactionsPerBlock import gate (kept for production/mempool). Commit 59bd7805.
- [x] D6 (M) OracleResponse::verify — all 5 C# checks implemented. Commit dd92087f.
- [x] D7 (M) Accept uncompressed 65-byte ECPoints in witness groups + Signer.allowed_groups (decode→compressed). Commit b099e429.
- [x] D8 (S) Removed non-C# StorageItem.is_constant; StorageValue serializes raw value bytes. Commit a03c678a.

### Batch E — dBFT block assembly (neo-consensus + neo-node)
- [ ] E1 (M) NextConsensus from **next-block** validator set (ShouldRefreshCommittee ? compute : get) `neo-node/src/consensus.rs:951`, `neo-consensus/.../commit.rs:205`.
- [ ] E2 (S) load_validators: always use get_next_block_validators for signers; refresh-conditional set only for NextConsensus `neo-node/src/consensus.rs:297-312`.

### Batch F — RPC/JSON interop (neo-rpc + neo-json)
- [x] F1 (S) getunclaimedgas returns raw datoshi string. Commit c69a300c.
- [x] F2 (S) getrawtransaction verbose: dropped extra `vmstate` field. Commit c69a300c.
- [x] F3 (S) Relay error path: `with_data(reason)` on all arms. Commit c69a300c.
- [x] F4 (M) RPC JSON escaping = C# JavaScriptEncoder.Default (new neo-json::escape CSharpEscapeFormatter; routes RPC + JToken through it). Commit 4533d0d1. FOLLOW-UP: jsonrpsee + WebSocket transports not yet routed through it.
- [x] F5 (S) Deleted dead neo-p2p RawMessage (divergent compression, unused). Commit de6bf097.

### BONUS consensus fix (surfaced by A1) — commit 84dc18b7
- [x] `NativeContract::is_active` used `unwrap_or(0)` → unscheduled ActiveIn hardfork treated as active-from-genesis. Now delegates to `settings.is_hardfork_enabled` (false for unscheduled), matching C# `NativeContract.IsActive` + `is_initialize_block`. Was latent until Faun unscheduled; would have wrongly activated Treasury/TokenManagement (state-root divergence). **Lesson: run the full server-feature suite, not just targeted tests.**

---

## PHASE 4 — Architecture & crate boundaries

### Batch G — Duplication & dead code (low risk, high clarity)
- [ ] G1 (S) Delete neo-core `compression/` facade (23 LOC) → callers use neo-io directly.
- [ ] G2 (S) Delete dead `neo-io/src/extensions/binary_reader.rs`+`binary_writer.rs` (3rd var-int copy, no call sites) — confirm first.
- [ ] G3 (S) Delete dead `neo-storage/src/persistence/index.rs` (secondary-index, no C# counterpart) — confirm first.
- [ ] G4 (S) persistence/serialization.rs: delete misleading bincode `serialize/deserialize` (dead, falsely "matches C#").
- [ ] G5 (S) Collapse neo-core/persistence re-export shims; single canonical import path (keep StorageItemExt).
- [ ] G6 (M) Collapse neo-rpc duplicate `expect_u32/hash_param` helpers; pick one param system (delete trait converter or route all through it).

### Batch H — Telemetry consolidation
- [ ] H1 (S) Delete neo-core/src/telemetry (dead inside neo-core); neo-node uses only neo-telemetry. Move lightweight primitives into neo-telemetry if core needs them.
- [ ] H2 (M) Unify metrics on one prometheus Registry (or `metrics` facade); remove hand-rolled AtomicU64 Counter/Gauge.

### Batch I — ProtocolSettings unification
- [ ] I1 (L) Single ProtocolSettings: neo-config canonical (ECPoint committee, Hardfork map), neo-core re-exports; neo-config defaults read from neo-primitives::constants. Cross-crate equality test in CI.

### Batch J — Storage backend placement
- [ ] J1 (M) Move RocksDB provider + write_batch_buffer from neo-core/persistence into neo-storage behind `rocksdb` feature (use StorageError). Removes ~1.3K LOC + rocksdb dep from neo-core.

### Batch K — Actor runtime
- [ ] K1 (M) Extract neo-core/src/actors → `neo-actors` crate (zero neo deps); neo-core re-exports as `runtime`; neo-rpc depends on neo-actors directly.
- [ ] K2 (L, incremental) Simplify toward tokio task+typed-channel; first cheap step: delete DefaultMailbox drain-into-VecDeque, use biased select! over system+user mpsc.

### Batch L — Consensus runtime placement
- [ ] L1 (L) Move ConsensusActor + DbftConsensusController + WalletConsensusSigner from neo-node/src/consensus.rs → neo-consensus::runtime. Node keeps only wiring. (Do AFTER E1/E2.)

### Batch M — neo-rpc decomposition
- [ ] M1 (M) Decouple `server` feature from `client` (don't compile reqwest+SDK for the node); shared models module.
- [ ] M2 (M) Block/Header `to_json(&ProtocolSettings)` in neo-core; RPC server + client consume it (kills 3-way drift; pairs with F4).
- [ ] M3 (L) Migrate RPC transport warp → jsonrpsee (already a dep; removes hyper 0.14 tree); validate response byte-shape first. Behind `server` feature.
- [ ] M4 (L, later) Split neo-rpc → neo-rpc-client / neo-rpc-server / neo-rpc-types.

### Batch N — P2P ownership (large, decide direction)
- [ ] N1 (L) **Decision:** either promote neo-p2p to own the full networking stack (move local_node/remote_node/task_manager/connection/framed/handshake from neo-core/network/p2p) introducing a trait seam for ledger payloads, OR rename neo-p2p → neo-p2p-types and fix its doc. Default: continue stateless-type migration; defer the big move.

### Batch O — Inlined plugin extraction (large, lowest priority; deeply ledger-coupled)
- [ ] O1 (L) application_logs → neo-application-logs (lowest coupling first).
- [ ] O2 (L) tokens_tracker → neo-tokens-tracker.
- [ ] O3 (L) state_service → neo-state-service.
- [ ] O4 (L) oracle_service → neo-oracle-service.
- [ ] O5 At minimum: feature-gate all four so a minimal node need not compile them.

### Batch P — Build hygiene (low effort)
- [ ] P1 (S) Move tonic-build/protoc build-deps out of always-compiled neo-core (sub-crate or xtask) — gate to neofs-grpc.
- [ ] P2 (S) Workspace tokio: default-features=false + per-crate feature opt-in (follow neo-core pattern).

---

## KEEP AS-IS (audit-confirmed correct — do NOT touch)
- neo-crypto: idiomatic RustCrypto/blst, no hand-rolled primitives.
- UInt160/UInt256 word layout + Ord (consensus-correct vs C#).
- neo-tee / neo-hsm: correctly isolated optional crates (depend on neo-crypto only) — template for optional subsystems.
- DataCache/StoreCache/Trackable layering in neo-storage.
- neo-core/network payloads (Block/Tx/Signer/Header) staying in core (genuine ledger coupling, C#-faithful).
- P2P framing (tokio-util codec), rate limiting (governor), CLI (clap), config (toml+serde), shutdown (tokio signals + CancellationToken).
- Do NOT introduce libp2p (incompatible with Neo's TCP wire protocol). Do NOT swap rocksdb speculatively. Do NOT add figment/config (YAGNI).

## Decisions made by user (2026-05-30)
1. **VM engine (B5)** — KEEP both engines; ALIGN the external neo-vm-rs to C# (C# is the standard). CRITICAL CAVEAT: neo-vm-rs is multi-platform — it must keep working for **neo-riscv-vm and zkvm**. So **check neo-vm-rs before changing**; the constrained limits (MAX_CALL_DEPTH=64 fixed array, MAX_ITEM_SIZE=1MB const) may be deliberate for zk/riscv. Make limits **configurable per-context** (thread ExecutionEngineLimits through the semantics layer) so the Neo consensus path uses C# values (131070 item, 1024 depth, ReferenceCounter-based MaxStackSize) while zk/riscv keep their constrained limits. Implement C#-equivalent ReferenceCounter for the Neo path. Verify zk/riscv builds/tests still pass.
2. **P2P ownership (N1)** — MOVE the full networking stack (local_node, remote_node, task_manager, connection, framed, handshake, message handlers) from neo-core/network/p2p INTO neo-p2p. Introduce a trait seam for ledger-coupled payloads (Block/Tx/Header) since neo-p2p must not depend on neo-core. Large refactor; do as dedicated effort.
