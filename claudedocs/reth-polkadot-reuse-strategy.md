# Reusing reth + polkadot-sdk to build a Neo N3 v3.9.1-compatible Rust full node

> Engineering strategy for the neo-rs workspace. Audience: the maintainer mid-way through dissolving the `neo-core` monolith into layered single-responsibility crates. Written to be honest about a modest reuse ceiling, not to oversell.

---

## 1. Executive summary

**The core truth: reth and polkadot-sdk do not implement Neo's protocol, and nothing in either codebase can be made to.** They implement *Ethereum* and *Substrate/Polkadot* protocols respectively. Neo N3's consensus is defined byte-for-byte by the canonical C# node (v3.9.1), and the decisive, transitive disqualifier is **cryptographic and codec identity**:

- Neo's **primary signature curve is secp256r1 (NIST P-256)** paired with SHA256 — not Ethereum's secp256k1+keccak, not Substrate's sr25519/ed25519+blake2.
- Neo's wire/disk codec is a **little-endian Bitcoin-style var-int format** (`0xFD`+u16LE / `0xFE`+u32LE / `0xFF`+u64LE) — not RLP, not SCALE.

Because reth/alloy's *entire* stack (primitives, RLP wire, networking, trie, EVM, consensus) assumes secp256k1+keccak+RLP as the chain identity, and Substrate's stack assumes sr25519+blake2+SCALE, **both ecosystems are inapplicable at every layer that touches consensus.** Every transaction, block, consensus message, MPT node, and address re-serializes through Neo's exact codec and hashes with Neo's exact composition; any byte difference forks the chain.

**Honest reuse ceiling: roughly 15–20% of the node by surface area (and a smaller fraction by line count) — the infrastructure perimeter only.** What can run on external Rust infra without touching parity:

- the KV storage **backend** (an MDBX provider as an *alternate*, behind the existing `neo-storage` traits),
- the RPC **transport** (`jsonrpsee`, replacing `warp`+`hyper`),
- the **metrics** facade (`metrics` + `metrics-exporter-prometheus`),
- node **task-lifecycle** conventions (reth-tasks-style shutdown/panic propagation),
- **CLI** organization (clap with sc-cli/reth-cli decomposition patterns).

Everything that *defines the chain* stays custom: primitives/UInt encoding, the secp256r1+SHA256 crypto composition, the var-int codec (`neo-io`), P2P framing + command set (`neo-p2p`), dBFT (`neo-consensus`), NeoVM + gas (`neo-vm-rs`), the 11 native contracts, the MPT node encoding + state root, witness/template/fee logic, tx/block hashing, NEP-6/NEP-2 wallets, and the mempool/ledger validation rules.

The most valuable external leverage is **architectural patterns, not code**: reth-node-builder/ComponentsBuilder composition, reth-stages staged-sync (unwind/checkpoint) as a sync *driver*, reth-provider ProviderFactory layering, and sc-service TaskManager orchestration — all useful to guide the in-progress `neo-core` dissolution. The protocol crates themselves inherit **zero external code**; they already sit on the correct raw primitive crates (`p256`, `sha2`, `ripemd`, `blst`, `num-bigint`) and generic substrate (`tokio`, `bytes`, `lz4_flex`), all vendored today.

---

## 2. The hard boundary — subsystems that MUST stay Neo-specific

These cannot adopt any reth/polkadot **protocol** implementation. The reason is always the same shape: a Neo-defined byte layout, hash composition, opcode/gas semantic, or economic rule that must equal C# v3.9.1 exactly.

| Subsystem | Neo-rs home | Why it's protocol-bound (C# parity reason) | Parity risk |
|---|---|---|---|
| **Binary serialization (var-int / ISerializable)** | `neo-io/src/var_int.rs` | LE Bitcoin var-int (`<0xFD` single byte, else `0xFD`+u16LE / `0xFE`+u32LE / `0xFF`+u64LE). Every tx/block/consensus/MPT hash re-serializes through this. Explicitly **not** RLP, **not** SCALE. | breaks-parity |
| **Crypto curve + hash composition** | `neo-crypto/src/{hash,ecc,signature}.rs` | Primary curve secp256r1; `Hash160=RIPEMD160(SHA256)`, `Hash256=SHA256(SHA256)`, `IVerifiable.Hash`=single SHA256 over unsigned form, `GetSignData = network_magic(u32 LE) ‖ hash`. secp256k1+keccak and ed25519+BLS exist **only** as secondary CryptoLib methods (see §5). | breaks-parity |
| **P2P wire format + framing** | `neo-p2p`, `neo-core/src/network/p2p/framed.rs` | Frame `[Flags:u8][Command:u8][VarBytes payload]`, `PayloadMaxSize=0x02000000`, **LZ4** (not snappy). `MessageCommand` bytes fixed (`Version=0x00`, `Verack=0x01`, `Inv=0x27`, `Block=0x2c`, `Extensible=0x2e`, …). | breaks-parity |
| **dBFT 2.0 consensus** | `neo-consensus` | `ConsensusMessage` = `[Type:u8][BlockIndex:u32LE][ValidatorIndex:u8][ViewNumber:u8]` + body over `ExtensiblePayload`; view-change/prepare/commit/recovery state machine; validator ordering; `M=2f+1`. GRANDPA/BABE/eth-PoS cannot host it. | breaks-parity |
| **NeoVM + gas metering** | `neo-vm-rs` (semantics) + `neo-core/src/neo_vm` (host) | Distinct stack machine: own opcode set, StackItem type system, ref-counting limits, per-opcode + syscall GAS table, deterministic fault/halt, exact `FeeConsumed` (consensus-bound). revm/pallet-contracts unusable. | breaks-parity |
| **The 11 native contracts** | `neo-core/src/smart_contract/native` | Fixed contract hashes, method ABIs, storage key layouts, economics (NEO/GAS issuance, committee voting, gas-per-block, fee factors, oracle). Highest consensus risk. No reth-precompile / Substrate-pallet analogue. | breaks-parity |
| **MPT node encoding + state root** | `neo-crypto/src/mpt_trie`, `neo-core/src/state_service` | Node tags `Branch=0x00` (17 slots), `Extension=0x01`, `Leaf=0x02`, `HashNode=0x03`, `Empty=0x04`; SHA256-over-ISerializable node hashing; ref-counts; **no** HP/compact-nibble encoding. State root is consensus-relevant. | breaks-parity |
| **Tx / block structure + hashing** | `neo-core/src/ledger`, neo-p2p payloads | Fixed field order; tx/block hash = single SHA256 over *unsigned* serialization; SHA256 MerkleRoot; Witness/Signer/WitnessScope trees with fixed type tags. | breaks-parity |
| **Witness / script verification** | `neo-core` (smart_contract helper) | Runs verification script in NeoVM under `Verification` trigger with `MaxVerificationGas=1.5`; recognizes `IsSignatureContract`/`IsMultiSigContract` templates; fixed `SignatureContractCost`/`MultiSignatureContractCost`; script hash = Hash160 of verification script. | high |
| **Address format** | `neo-crypto/src/encoding.rs`, `neo-primitives/src/base58_check.rs` | Base58Check over `[AddressVersion=0x35 ‖ scriptHash(20)]`, checksum = first 4 bytes of `SHA256(SHA256(...))`. Differs from eth (keccak, EIP-55) and SS58 (blake2). | high |
| **Mempool** | `neo-core` ledger/`memory_pool`, `neo_system/mempool.rs` | Verification, eviction, sorting, capacity, re-verification must match C# `MemoryPool` (fee / ValidUntilBlock / conflict model). reth nonce/EIP-1559 pool is foreign. | high |

**The single sentence to remember:** the curve choice (secp256r1) plus the bespoke var-int codec transitively disqualify both external ecosystems at the protocol layer, because every one of their primitives/codecs/networking/consensus crates assumes their native curve and codec.

---

## 3. Reuse map

Recommendation legend: **reuse-direct** (depend on the crate as-is) · **adapt** (depend, but wrap/migrate with parity gates) · **inspire** (copy the design, take no dependency) · **keep-custom** (no external leverage).

Adversarial corrections are folded in: storage and metrics framings are corrected, jsonrpsee is downgraded from "promote to default" to "make it the `server`-feature implementation behind a parity gate," and reth-tasks is held strictly inspire-only and deferred.

| Subsystem | Recommendation | Concrete crate | Replaces in neo-rs today | Protocol risk | Effort |
|---|---|---|---|---|---|
| Storage **backend** (KV engine) | **adapt** | `libmdbx`/`heed` (prefer raw bindings over `reth-libmdbx` to avoid reth crate-graph pins) | *Adds* an MDBX provider **alongside** `neo-core/src/persistence/providers/rocksdb`; RocksDB stays default | none — **but commit/snapshot semantics are parity-adjacent** | **M+** |
| Storage typed-table layer | **inspire** | `reth-db-api` Table/Encode/Decode/Compress *pattern only* | Informs `neo-storage` key_builder/store_cache if refactored | low (StorageItem encoding is parity-relevant) | S |
| Storage abstraction / DataCache / snapshots | **keep-custom** | — | `neo-storage` + `neo-core/src/persistence` | low (commit ordering / snapshot isolation are consensus-visible) | S |
| RPC **server transport** | **adapt** | `jsonrpsee` 0.24/0.26 directly (**not** reth-rpc-builder / sc-rpc-server) | Make jsonrpsee the impl of the opt-in `server` feature; retire `warp` 0.3 + `hyper` 0.14 **only after transport-parity** | low — **conditional on a byte-level RPC parity gate** | **L** |
| RPC **client** | **keep-custom** | `reqwest` 0.12 (already idiomatic) | `neo-rpc` client | low (must match C# result schemas; tolerate string-encoded large ints) | S |
| Telemetry / metrics | **adapt** | `metrics` + `metrics-exporter-prometheus` | **Retire `neo-telemetry/src/facade/recorder.rs`** (the dashmap facade) — *not* `metrics.rs`/`node_metrics.rs`, which are prometheus-typed | none (consensus); **low operational** (names/labels/buckets) | **S–M** |
| Task runtime / actors | **inspire** (deferred) | `reth-tasks` conventions only (crate is **absent** today) | Mirror shutdown/panic propagation **in `neo-actors`**; keep the actor runtime | none | M |
| CLI / node-binary | **inspire** | `clap` 4.5 (kept) + sc-cli SharedParams + reth-cli runner skeleton | Informs `neo-node` cli/startup as subcommands grow | none | S |
| Primitives / hash-types (UInt160/256, BigDecimal) | **keep-custom** | `neo-primitives` (already `[u8;20]`/`[u8;32]` newtypes) | nothing | high | S |
| Crypto composition | **keep-custom** | already on `p256`/`k256`/`secp256k1`/`ed25519-dalek`/`blst`/`sha2`/`ripemd`/`sha3`/`blake2` | nothing | breaks-parity | S |
| Binary serialization (`neo-io`) | **keep-custom** | only `bytes`/Buf/BufMut beneath | nothing | breaks-parity | S |
| P2P transport (framing/handshake) | **keep-custom** | `tokio` + `tokio-util` Framed + `lz4_flex` (substrate only) | nothing wholesale (this layer is actively being refactored) | high | M |
| P2P wire format (`neo-p2p`) | **keep-custom** | — | nothing (canonical) | breaks-parity | S |
| Mempool | **keep-custom** | `indexmap`/`dashmap`/`parking_lot` (right primitives already) | nothing | high | S |
| Ledger / blockchain | **keep-custom** (borrow reth-stages driver only) | `rayon` for bulk sync | nothing | breaks-parity | S |
| Consensus / dBFT | **keep-custom** | `neo-actors` mailbox, timers (already present) | nothing | breaks-parity | S |
| VM (NeoVM + ApplicationEngine) | **keep-custom** | `num-bigint` backs Integer | nothing (B3/B4/B5 limit alignment is *internal* work) | breaks-parity | M |
| Native contracts | **keep-custom** | CryptoLib delegates to `p256`/`k256`/`ed25519-dalek`/`blst` internally | nothing | breaks-parity | S |
| MPT / state-root | **keep-custom** | RocksDB/MDBX + LRU as the only reusable backing | nothing in node encoding | breaks-parity | M |
| Wallets (NEP-6 / NEP-2) | **keep-custom** | `bip39`/`bs58`/`scrypt`/`aes` (right primitives) | nothing | high | S |

---

## 4. Quick wins (ordered, low-risk first)

Do these in order. The first three are the genuine, parity-safe leverage; the rest are pattern borrows.

### 4.1 — Consolidate telemetry onto one facade *(start here — lowest risk, smallest surface)*
- **Lands in:** `neo-telemetry`.
- **The honest scope (corrected):** the bespoke piece to retire is **`neo-telemetry/src/facade/recorder.rs`** (a `DashMap`-backed counter/histogram facade with its own `to_prometheus_text()`/`to_json()`), **not** `metrics.rs`/`node_metrics.rs` — those are already built on the typed `prometheus` crate. The real problem is that **two metrics systems already coexist**; adopting `metrics` + `metrics-exporter-prometheus` is only a win if it *subsumes* `recorder.rs` rather than becoming a third system.
- **Why parity-safe:** zero consensus coupling. Only external call sites are in `neo-node` (`metrics.rs`, `health.rs`), so the "re-instrument every call site" cost is small — effort is closer to **S** than M.
- **Acceptance gate:** diff `/metrics` text output before/after; pin histogram bucket boundaries, metric names, and labels so dashboards/alerts don't silently break.

### 4.2 — Add MDBX as an *alternate* storage provider
- **Lands in:** `neo-core/src/persistence/providers/` (a new `mdbx/` sibling to `rocksdb/`) — **not** `neo-storage`. The trait surface lives in `neo-storage/src/persistence` (`Store` / `ReadOnlyStore` / `ReadOnlyStoreGeneric` / `WriteStore` / `StoreSnapshot` / `StoreProvider`), but the concrete adapter sits in `neo-core` next to RocksDB.
- **Crate:** prefer raw `libmdbx`/`heed` bindings over `reth-libmdbx` to avoid inheriting reth's workspace version pins and to simplify the license-header audit.
- **Why parity-safe:** on-disk key layout stays Neo-defined; the traits operate on `Vec<u8>`/`StorageKey`/`StorageItem` with no eth types crossing the boundary.
- **The hidden cost (corrected):** `StoreSnapshot::snapshot()`/`try_commit()` maps cleanly to RocksDB read-snapshots but to **MDBX MVCC write-transactions with single-writer serialization** — non-trivial binding work, and commit ordering / snapshot isolation are **consensus-visible** (a mismatched adapter produces a state-root divergence that *looks* like a parity break).
- **Acceptance gate:** `mainnet_state_roots_vs_csharp` and `mainnet_block_*_repro` must pass **identically on both backends**. Treat as **M+**, and sequence it with the `neo-core` dissolution (the provider lands inside the code being moved).

### 4.3 — Standardize the RPC server on jsonrpsee *(largest, gate carefully)*
- **Lands in:** `neo-rpc`. A 116-line `jsonrpsee_adapter.rs` already exists behind the `jsonrpsee-server` feature.
- **Corrected framing:** `neo-rpc` has `default = []` (no default server) and the `jsonrpsee-server` feature is currently **additive over `warp`+`hyper`**, not a replacement. So this is *not* "flip a flag to default." The method-dispatch logic (≈18.4k LOC across the handlers, via `invoke_rpc_handler`/`resolve_rpc_handler`) is already transport-agnostic and **shared** with warp — that part carries low risk.
- **The actual L-effort and risk:** the **warp transport surface the adapter does not yet replicate** — custom rustls TLS termination with trusted-authority client-cert auth (`rpc_server.rs`), the WebSocket `WsEventBridge` (`ws/handler.rs`), CORS (`routes/cors.rs`), gzip, and `BasicAuth` gating (the adapter only registers non-auth public methods). TLS/client-cert/auth/WS-subscription semantics are operational-security and behavior parity surfaces.
- **Why parity-conditional-safe:** JSON-RPC method names, param coercion (note `serde_urlencoded` query-style params), error envelopes, and result encoding (hex/`0x`/large-int-as-string) must stay byte-identical to C# `RpcServer`.
- **Acceptance gate:** a golden-response corpus captured from C# `RpcServer` (methods + errors + batches) asserting byte-identical output, **plus** transport feature-parity (TLS, auth, CORS, WS) — **then** flip the default. Keep warp as default until both pass.

### 4.4 — Mirror reth-tasks shutdown/panic conventions *in neo-actors* (defer)
- **Lands in:** `neo-actors` (do **not** add the `reth-tasks` crate — it's absent from the tree, so adopting it is net-new dependency surface, not consolidation).
- **Why parity-safe:** purely lifecycle/ergonomic; `reth-tasks` must never touch consensus/mempool/ledger actor message ordering.
- **Sequencing:** **defer until the `neo-core`/P2P dissolution settles** — rewiring spawn/shutdown call sites against actively-refactored code is churn against a moving target. Borrow the *conventions* (`spawn_critical`, panic propagation, graceful `Shutdown`) into `neo-actors` itself.

### 4.5 — Borrow CLI decomposition patterns *(cleanest of all)*
- **Lands in:** `neo-node` (already on `clap` 4.5 + `mimalloc`).
- **Pattern:** sc-cli's `SharedParams`/`DatabaseParams`/`NetworkParams`/`RpcParams` struct decomposition + reth-cli-runner's CLI→Config→launch dispatch, as subcommands grow (`init`/`import`/`db`/`node`/`stage`). **No dependency on sc-cli/reth-cli** (both carry framework type coupling).
- **Why parity-safe:** CLI is implementation-only — but consensus/network defaults (network magic, seed peers) must come from **Neo config**, never borrowed framework defaults.

### 4.6 — Keep using the already-correct primitive crates
Not a change, but the real safe reuse, present today: `p256` (secp256r1), `sha2`/`ripemd`, `bs58`, `num-bigint`, `lz4_flex`, `bytes`/`tokio-util`. **Do not** pull `alloy-primitives` Address/eth hashers, `parity-scale-codec`, `sp-core`, `reth-trie`, or any reth/Substrate protocol crate.

---

## 5. Explicitly NOT reusable

Tempting because they sound generic; each one breaks parity for a precise reason.

| Tempting reuse | Why it breaks parity (one line) |
|---|---|
| **`reth-trie`** for the Neo MPT | Eth trie is keccak-over-RLP with HP/compact-nibble keys; Neo is SHA256-over-ISerializable, 17-arity branch, `HashNode`/`Empty` sentinels, ref-counts — a different node format. |
| **`trie-db` (paritytech)** for the Neo MPT | `TrieLayout` abstracts Hasher+NodeCodec, but Neo's 17-arity + sentinel kinds + ref-counting + SHA256-over-ISerializable force a full NodeCodec rewrite — zero leverage over the hand-written trie. |
| **`sc-network` / `libp2p`** for Neo P2P | Neo is cleartext length-framed TCP (`[Flags][Command][VarBytes]`, LZ4); libp2p substreams/multiplexing are a different byte format. |
| **`reth-eth-wire` (RLPx/ECIES)** for Neo P2P | Encrypted RLPx handshake and Eth subprotocol command set are wire-incompatible with Neo's cleartext frame + `MessageCommand` bytes. |
| **`k256` / `secp256k1` as the chain's identity curve** | Neo's identity curve is **secp256r1**; secp256k1 exists *only* as the discrete `CryptoLib.recoverSecp256K1` native method (HfEchidna). Using it as the signing default forks consensus. |
| **keccak256 everywhere** (alloy/reth assumption) | Neo hashes with SHA256/RIPEMD160; keccak exists *only* as the `CryptoLib.keccak256` native method (HfCockatrice). It is **not** the chain's general hash. |
| **RLP** | Neo serialization is LE Bitcoin var-int, not RLP; every hash would change. |
| **SCALE / `parity-scale-codec`** | Same — not SCALE; field order + var-int markers are Neo-specific. |
| **`alloy-primitives` Address / `primitive-types` H160/H256** | Their big-endian `Display`/`Ord`/serde leak vs Neo's reversed-hex convention and version-`0x35` Base58Check address. Neo's inner storage is already `[u8;N]`; there is **no leverage, only parity surface**. (Confirmed absent from the lockfile — keep it that way.) |
| **`revm` / `reth-evm`** | EVM is 256-bit-word, keccak-storage, eth gas schedule; NeoVM is a StackItem machine with its own opcodes and GAS table. |
| **`pallet-contracts` (Wasm/ink!)** | Different instruction set, metering, and host interface from NeoVM. |
| **GRANDPA / BABE / AURA (`sc-consensus`, `finality-grandpa`)** | Cannot host dBFT 2.0's `ConsensusMessage` wire format, view-change state machine, or `M=2f+1` thresholds. |
| **reth eth tx-pool / Substrate transaction-pool** | Encode nonce/EIP-1559 fee-market / extrinsic semantics; Neo uses a fee / ValidUntilBlock / conflict model with no replace-by-fee analogue. |
| **`sp-keystore`** for wallets | sr25519-first and **lacks secp256r1**; NEP-6 JSON + NEP-2 (scrypt N=16384,r=8,p=8 + AES) must match C# exactly. |

> **Two precision notes the maintainer must internalize:**
> 1. secp256k1 and keccak256 are **not** "off the protocol path" — they are **consensus-relevant CryptoLib native methods** (`recoverSecp256K1` @ HfEchidna, `keccak256` @ HfCockatrice) whose outputs feed VM execution and thus state. Keep all four curve crates + sha3. reth/alloy is disqualified because it assumes secp256k1+keccak as *chain identity*, not because those primitives are unused.
> 2. **NEP-2 is AES-256 in ECB mode** over the two XORed 16-byte halves — *not* AES-CBC. If the rationale or code says CBC, that is a parity bug; verify the cipher mode in `neo-core/src/wallets` against C# `NEP2.cs`.

---

## 6. How this reshapes the neo-core decomposition

The dissolution target (layered single-responsibility crates, reth/polkadot-style) should treat external infra as a **thin replaceable shell around an immovable protocol core.** Concretely:

```
                  ┌─────────────────────── neo-node (binary) ───────────────────────┐
   PATTERNS ───►  │  CLI: clap + sc-cli/reth-cli decomposition (init/import/db/node) │
   (no dep)       │  Task lifecycle: reth-tasks-style conventions in neo-actors      │
                  └──────────────────────────────┬──────────────────────────────────┘
                                                 │
        ┌────────────── INFRASTRUCTURE PERIMETER (reuse-OK) ──────────────┐
        │  neo-rpc      → jsonrpsee transport (warp retired post-gate)     │
        │  neo-telemetry→ metrics + metrics-exporter-prometheus facade     │
        │  neo-storage  → trait surface (Store/ReadOnlyStore/WriteStore/   │
        │                 StoreSnapshot/StoreProvider)                     │
        │     ├─ providers/rocksdb (default)   ← in neo-core today         │
        │     └─ providers/mdbx    (alternate) ← lands next to rocksdb     │
        └─────────────────────────────────┬───────────────────────────────┘
                                          │  IStore-style boundary (Vec<u8>/StorageKey/StorageItem)
   ┌──────────────────── PROTOCOL CORE (keep pure, zero external code) ────────────────────┐
   │ neo-io (var-int)  neo-primitives  neo-crypto (secp256r1+SHA256 composition)            │
   │ neo-p2p (wire)    neo-consensus (dBFT)   neo-vm-rs (+ host)   native contracts          │
   │ neo MPT/state-root   ledger/mempool/tx-block hashing   witness/fee   NEP-6/NEP-2        │
   └───────────────────────────────────────────────────────────────────────────────────────┘
```

Decomposition rules that fall out of the reuse analysis:

1. **Keep the protocol crates dependency-clean.** `neo-io`, `neo-primitives`, `neo-crypto`, `neo-p2p`, `neo-consensus`, native contracts, the MPT, and the VM must depend only on raw primitive crates (`p256`, `sha2`, `ripemd`, `blst`, `num-bigint`) and generic substrate (`tokio`, `bytes`, `lz4_flex`). Forbid any reth/alloy/Substrate/scale/rlp/fixed-hash dependency by lint/CI on these crates' `Cargo.toml`.
2. **`neo-storage` is the seam for backend pluggability.** The trait surface (`Store`/`ReadOnlyStore`/`WriteStore`/`StoreSnapshot`/`StoreProvider`) already isolates the engine. The concrete adapters (`providers/rocksdb`, future `providers/mdbx`) currently live in `neo-core` — **decide during the dissolution whether the adapters move into `neo-storage`** or stay in a thin `neo-core` persistence layer. Either way, the MDBX provider's snapshot/commit semantics are a parity gate, not a free swap.
3. **`neo-rpc` standardizes on jsonrpsee for transport, keeps handlers transport-agnostic.** The `invoke_rpc_handler`/`resolve_rpc_handler` dispatch is the durable contract; warp and jsonrpsee are both just transports behind it. Preserve TLS/auth/CORS/WS behavior as first-class transport requirements, not afterthoughts.
4. **`neo-telemetry` collapses to one facade.** Resolve the existing two-system duplication (`prometheus`-typed `metrics.rs`/`node_metrics.rs` vs dashmap `facade/recorder.rs`) before — or as part of — adopting the `metrics` facade. Net goal: one instrumentation API, one `/metrics` exposition, stable names/labels/buckets.
5. **`neo-actors` stays the runtime; node-lifecycle conventions layer on top.** Do not rip out the actor model (consensus and the P2P local-node depend on it). Adopt reth-tasks-style graceful shutdown / panic propagation as conventions inside `neo-actors`, applied at the `neo-node` boundary.
6. **Pin the VM.** `neo-vm-rs` is a `../neo-vm-rs` **path sibling with its own `.git`** — not a workspace member or submodule — so parity-critical VM changes (the B3/B4/B5 limit alignment) are *not* reproducibly locked by this repo's lockfile/CI. **Vendor it as a git submodule or workspace member** so the VM's parity guarantees are version-locked with the node.
7. **Borrow reth-stages only as a sync driver.** The staged-sync pipeline shape (Pipeline/Stage/checkpoint/unwind) is a good organizing pattern for the bulk-sync *driver*; the stages themselves stay Neo-specific (`rayon` bulk sync remains). Keep per-block execute-then-stateroot ordering exactly as C# does.

---

## 7. Recommended next 3 concrete steps

| # | Step | Where | Rough effort | Why first |
|---|---|---|---|---|
| **1** | **Consolidate telemetry onto one facade.** Decide on a single backend; retire `neo-telemetry/src/facade/recorder.rs`; add a `/metrics` text-diff acceptance test (pin names/labels/buckets) before touching call sites. | `neo-telemetry`, `neo-node` | **S** (1–2 days) | Lowest risk, smallest call-site surface, zero parity coupling. Removes existing duplication (a real defect) rather than adding a third system. Builds the "diff-before-and-after" muscle the harder migrations need. |
| **2** | **Land the RPC parity-gate harness, then make jsonrpsee the `server`-feature implementation.** Capture a golden corpus from C# `RpcServer` (methods + errors + batches, byte-identical); reimplement TLS/client-cert/CORS/gzip/BasicAuth/WS-bridge on jsonrpsee; keep warp default until both serialization-parity and transport-parity pass. | `neo-rpc` | **L** (2–4 weeks) | Highest-value consolidation (both reth and Substrate converge on jsonrpsee), but the warp transport surface is the real cost and the parity risk. The harness is reusable for step 3. |
| **3** | **Add the MDBX alternate provider behind the existing storage traits, gated on dual-backend state-root reproduction.** Implement `providers/mdbx` mapping `StoreSnapshot` to MDBX MVCC write-txns (single-writer); prefer raw `libmdbx`/`heed`; require `mainnet_state_roots_vs_csharp` + `mainnet_block_*_repro` to pass identically on RocksDB and MDBX. Sequence with the `neo-core` persistence-layer move. | `neo-core/src/persistence/providers/mdbx`, `neo-storage` | **M+** (1–3 weeks) | Genuine backend leverage with zero on-disk-layout exposure, but commit/snapshot ordering is consensus-visible — the gate is mandatory. Slot it into the dissolution where the persistence adapters are being relocated. |

**Bottom line for the maintainer:** the realistic reuse from reth/polkadot is modest and confined to the infrastructure perimeter — transport, storage backend, telemetry, CLI, and task plumbing. That is worth doing (it removes real duplication and bespoke code), but it will not move the needle on the bulk of the work. The node's value *and* its risk live in the Neo-specific core, which must stay custom by the definition of 100% C# v3.9.1 parity. The best thing the external ecosystems give you is not code but **architectural patterns** to steer the `neo-core` dissolution — use those freely; depend on their protocol crates never.

---

## Addendum (2026-05-30): telemetry fold-in status + turnkey metrics swap

**Done (commit cd65e391):** retired the hand-rolled dashmap `Telemetry` facade
(~800 LOC) and consolidated neo-node onto the existing `prometheus`-crate stack.
This delivered the "remove our own code / use an existing solution" win.

**Deferred — `prometheus` crate → `metrics` + `metrics-exporter-prometheus`
(reth's stack).** Evidence shows this is a MULTI-CRATE migration, not single-crate:
the `prometheus` crate is used by BOTH `neo-telemetry` (node_metrics.rs global
registry + metrics.rs `Metrics`/`MetricsServer`) AND `neo-rpc/src/server/rpc_server.rs`.
Migrating only neo-telemetry would re-create a dual metric system or split `/metrics`.

Turnkey recipe (do as ONE coherent change, verify green before commit):
1. Add to workspace deps: `metrics` (0.24+), `metrics-exporter-prometheus` (0.16+).
   Confirm macro API: `metrics::gauge!("name").set(v)`, `metrics::counter!("name").increment(n)`.
2. New `neo-telemetry/src/recorder.rs`: `static HANDLE: OnceLock<PrometheusHandle>`;
   `fn install() -> &PrometheusHandle` via `PrometheusBuilder::new().install_recorder()`
   (idempotent — ignore AlreadyInstalled). `fn render() -> String { handle.render() }`.
3. node_metrics.rs: delete the 15 `LazyLock<Gauge/Counter>` statics + register_* helpers;
   rewrite `update_node_metrics/update_timeout_metrics/update_storage_metrics` to emit via
   `metrics::gauge!/counter!`; `gather_prometheus()` -> `recorder::render().into_bytes()`.
   FOOTGUN: exporter only renders metrics already emitted — emit a 0 for each gauge at
   install() so they appear before first update (matches current always-present behaviour).
4. metrics.rs: `Metrics`/`MetricsServer` -> serve `recorder::render()`; drop the private Registry.
5. neo-rpc/src/server/rpc_server.rs: migrate its `prometheus::` usage to `metrics::` macros
   + the shared recorder (so there is ONE exposition path).
6. Remove the `prometheus` crate dep from neo-telemetry + neo-rpc.
7. Verify: `cargo test -p neo-telemetry`, `cargo test -p neo-node metrics` (asserts the
   text contains neo_block_height / neo_p2p_timeouts_handshake / neo_state_roots_accepted),
   `cargo check -p neo-rpc --features server`, `cargo check --workspace --all-targets`.
