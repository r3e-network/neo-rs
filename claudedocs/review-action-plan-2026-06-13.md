# neo-rs whole-codebase review — action plan & status (2026-06-13)

> Historical review snapshot. Current composition uses typed service fields;
> references below to a `ServiceRegistry` describe the pre-ADR-034 tree.

Source: a 26-crate deep review across 9 quality dimensions (style, Rust best
practice, blockchain-node best practice, duplication, over-engineering, clarity,
correctness, efficiency, reinvented-wheels). Full per-crate findings +
synthesis: `claudedocs/per-crate-deep-review-2026-06-13.json`.

The dominant theme is **dead / parallel subsystems** left from the
pre-consolidation NeoSystem/actor architecture, plus stringly-typed boundary
errors, stale `neo-core` docs, and a handful of real correctness bugs.

## DONE (landed this session, each verified green)

Correctness:
- **Transaction wire size** — deleted an inherent `Transaction::size()` that
  shadowed `Serializable::size()` (version 4B not 1B, no script var-int prefix,
  witnesses omitted); `fee_per_byte()` (mempool ordering) + RPC `size` now use
  the true wire size. (neo-payloads)
- **BigDecimal::parse underflow** — trailing-zero amounts ("0.0"/"10.0"/"100.0")
  underflowed the usize decimal counter (panic/garbage scale); now trims only
  fractional zeros, matching C#. Regression test added. (neo-primitives)
- **find_paying_accounts panic** — guarded an empty-account `len()-1` underflow
  reachable from multi-output wallet transfers. (neo-rpc)
- **RPC startup deadlock** — `start_rpc_server` re-locked the `RwLock<RpcServer>`
  it already write-held while building the jsonrpsee module; the daemon hung on
  startup (RPC on by default). Fixed earlier in the session. (neo-rpc)

Dead code / over-engineering removed:
- `neo-rpc-types` zero-consumer shim crate (workspace 29→28 members).
- No-op `rpc_method!` macro + 2 dead `_macro` handler registrations.
- Dead `BinarySerializerError` enum (never constructed; misleading From impls).
- `neo-execution` interop-descriptor framework (`interop_descriptor.rs` +
  `interop_parameter_descriptor.rs`, ~402 LoC, zero callers).
- `neo-error::ToNativeError` trait (zero callers).
- `neo-storage::AutoFlushBatchBuffer` (never instantiated; leaked a detached,
  never-terminating background thread holding the DB handle).
- `neo-io::impl_from_bytes!` macro (never invoked).

Style / clarity / idiom:
- 7 hand-rolled `impl Default` → `neo_io::impl_default_via_new!`.
- `neo-tee` FcfsWithGasCap: `saturating_mul` to remove an i64 overflow panic.
- `neo-vm`: removed an uncontrolled debug `println!` on the VM fault path.
- `neo-runtime`: replaced a tautological `assert!(ok || !ok)` test line.
- `neo-consensus`: removed a duplicate `#![warn(missing_docs)]`.
- `neo-native-contracts`: dropped the no-op `#![allow(deprecated)]` (kept
  `#![allow(unused_imports)]` — it legitimately covers test-only `use super::*`
  imports).

## REMAINING — safe, mechanical (S/M, low risk) — good next batch
- **Stale `neo-core` module docs + 4 broken intra-doc links** (neo-network,
  neo-manifest, neo-payloads, neo-runtime). Breaks `cargo doc -D` CI. (#26)
- **Fictional crate-level doc examples** — neo-config `Settings::from_file`,
  neo-hsm `HsmRuntime`, neo-system `cancellation_token` doc vs shared-clone
  impl. Drop `ignore` so they compile-check. (#27)
- **Triplicated NEP-17 GAS balance decode** → one `GasToken::balance_of`
  (neo-native-contracts), called by neo-mempool + neo-rpc. (#19)
- **neo-tee Merkle root** → reuse `neo_crypto::MerkleTree` (it's a TEE ordering
  proof, not a consensus root — consolidate + differential test). (#33)
- **Unused deps sweep** (cargo-machete): neo-node (~dozen), neo-consensus
  (neo-system/neo-network/async-trait — a Layer-1 crate pulling network/system),
  neo-config, neo-runtime, neo-tee. (#25)
- **mimalloc**: wire `#[global_allocator]` or drop the half-configured dep. (#18)
- Misc clarity: neo-blockchain `_hash`→`hash` (handlers.rs:39, value is used);
  neo-state-service `state_root.rs` "double-SHA256" comment is wrong (code is
  correct single-SHA256 — fix the comment); neo-vm collapse byte-identical
  `Slot::clear`/`clear_references` and the 16 copy-paste conditional-jump
  handlers; neo-hsm misleading `let _ = &self.device_info;`.

## REMAINING — larger over-engineering deletions (M/L, compiler-verified safe)
Each is a public-but-zero-consumer subsystem; delete (or wire) behind its own
change since they touch core crates and need careful re-export cleanup:
- **neo-state-service verification pipeline** (StateStore/Verifier/commit-handler
  no-op/SyntheticStateRootCalculator/metrics) — only `MptStore` is live. Decide:
  wire commit handlers into the persist pipeline, or delete the dormant half.
  Directly enables the RPC `getstateroot`/`getstateheight` fix (#10/#11).
- **neo-execution legacy gas** — `consume_gas`/`check_gas`/`add_gas` mutate
  picoGAS vs datoshi (off-by-FEE_FACTOR), zero callers; delete. (#20 tail)
- **neo-vm parallel gas-metering surface + ~12 never-constructed VmError
  variants** — trim to what ApplicationEngine actually uses. (#21)
- **neo-network TaskManagerService + typed-wire (ProtocolMessage/NetworkMessage)**
  — NOTE: `TaskManagerService` has an integration test (intentional WIP sync
  scaffolding), so this is a wire-or-delete *decision*, not pure dead code. (#22)
- **Foundation dead modules** — neo-primitives `Fixed8`+`bigdecimal` dep + dead
  VM-limit consts (MAX_ARRAY_SIZE/MAX_ITEM_SIZE = MAX_BLOCK_SIZE: a latent
  consensus trap — highest-value here); neo-storage `persistence/index.rs`;
  neo-config global `HardforkManager` singleton. (#31)

## REMAINING — `Result<_, String>` → typed `CoreResult` (L, mechanical) (#24)
Public/boundary APIs against CONVENTIONS.md in: neo-manifest (~8 parsers),
neo-serialization (BinarySerializer/JsonSerializer), neo-wallets
(bip32/version/WalletFactory), neo-system TxRouterHandle, neo-crypto
NodeType::from_byte. Do per-crate with crate-internal thiserror types.

## REMAINING — node hardening (M, medium risk — touches live paths, no wire bytes)
- **RPC DoS limits inert** (#8, severity critical/operational): RpcServerConfig
  (max body/connections/batch, timeouts, CORS, rate-limit, auth) is parsed but
  never applied to the jsonrpsee server; GovernorRateLimiter is never invoked.
  Wire config + a tower/jsonrpsee middleware layer.
- **Mempool capacity** bounds only the verified queue, not total occupancy (~2×
  memory). Evict global lowest-fee preferring unverified until total ≤ cap. (#6)
- **LedgerContext is unbounded in RAM** (full blocks+headers, never evicted) →
  OOM on mainnet sync. Bound with an LRU of the last N; resolve cold reads from
  the durable store. (#7)
- **Network broadcast fan-out** `.await`s per-peer sends in the single command
  loop → one slow peer stalls the loop. Use `try_send`/spawn. (#9)
- **TxRouterHandle::try_enqueue_preverify** silently drops oracle-response txs
  (always Ok, no relay). Wire to mempool+broadcast or return Err. (#5)

## REMAINING — consensus-sensitive (HIGH risk — REQUIRE C# differential vectors
before any change; do NOT blind-edit)
- **NEP-2 verification-script salt** uses raw `System.Crypto.CheckWitness` ASCII
  instead of the canonical CheckSig redeem script → '6P…' keys not interoperable
  with standard wallets (round-trip test passes only because both sides share
  the wrong helper). `KeyPair::get_verification_script` already does it right;
  reuse it + a published NEP-2 known-answer vector. (#4)
- **CheckMultisig** returns Err (faults) where C# catches ArgumentException and
  returns `false` for invalid-but-decodable keys/sigs — can change tx/block
  verification outcome. (#13)
- **JSONPath evaluator** (Oracle filter) handles only a subset and omits C#'s
  maxDepth=6/maxObjects=1024 DoS bounds; filter comes from on-chain
  OracleRequest → consensus divergence + DoS. Port full C# grammar+guards. (#14)
- **NEF deserialize** lacks empty-script rejection + MaxItemSize cap on read
  (deploy path) vs C#. (#15)
- **ContractAbi::get_method** returns the LAST same-named overload for pcount=-1
  where C# returns the FIRST (affects `verify` resolution). (#16)
- **Consensus dedup-before-verify**: process_message marks an ExtensiblePayload
  seen before witness verification → forged-witness payload can evict the
  genuine one. Verify witness first, or document+test the relay-seam precondition
  (do not change the dedup hash bytes). (#17)
- **PKCS#11 HSM signing** returns raw r‖s without low-s normalization → may fail
  Neo verification; normalize like the rest of neo-crypto. (#32)
- **RocksDbSnapshot shares the live read cache** (point-in-time isolation
  hazard); give each snapshot its own cache. rocksdb is an optional backend. (#29)

## DO-NOT-TOUCH (reviewer-confirmed correct / intentional)
neo-node shared-DataCache startup snapshot (CoW Arc — re-binding would break it);
neo-state-service single-SHA256 `hash()` (C#-correct; only the comment is wrong);
neo-mempool witness/multisig verify + single-write-lock try_add; neo-p2p
WitnessCondition nesting/ECPoint/capability parity behaviors; neo-payloads
MerkleBlock Size-vs-Serialize quirk; neo-vm two-tier engine + ported subtle
semantics; neo-crypto malleability normalization / BLS / NamedCurveHash;
neo-system ServiceRegistry TypeId `.expect()`s; neo-consensus dbft signing path;
neo-serialization CSharpEscapeFormatter; neo-wallets NEP-2 AES/Base58Check crypto.

---

## Completion status — 2026-06-13 (continued execution)

### DONE this session (committed, each verified green)
- **#16 ABI overload** — `ContractAbi::get_method` returns the FIRST overload for
  pcount<0 (C# FirstOrDefault). [`14dc65b8`]
- **#4 NEP-2 verification-script salt** — uses the canonical CheckSig redeem
  script; '6P…' keys now interoperate with standard wallets + round-trip test.
  [`14dc65b8`]
- **#13 CheckSig/CheckMultisig** — out-of-field pubkey coordinate (≥Q) →
  `Ok(false)` (C# ArgumentException), other decode failures still fault.
  [`14dc65b8`]
- **#17 consensus dedup-before-verify** — payload marked seen only after the
  witness-verifying handler succeeds (anti cache-poison). [`14dc65b8`]
- **#15 NEF deserialize** — script read capped at MaxItemSize, empty/oversize
  rejected, total-size verified. [`14dc65b8`]
- **#29 RocksDbSnapshot** — ATTEMPTED then REVERTED [`16658fc5`]: removing the
  shared read cache regressed commit-invalidation coherency (a committed update
  stopped being visible to a new reader). Moved back to the remaining list; the
  isolation needs a coherency-preserving fix, not a cache removal.
- **#32 PKCS#11 low-s** — `Secp256r1Crypto::normalize_low_s`; HSM signer
  canonicalizes r‖s. [`14dc65b8`]
- **#14 Oracle JSONPath** — full C# JPathToken grammar ($.., slices, unions,
  negative indices, quoted keys) + maxDepth=6/maxObjects=1024 DoS bounds + 5
  differential tests from `UT_JPath.cs`. [`a9c59c9f`]
- **#20/#31 foundation deletions** — dead `Fixed8`/bigdecimal, dead
  MAX_ARRAY_SIZE/MAX_ITEM_SIZE consts, `persistence/index.rs`, zero-caller
  unit-inconsistent `check_gas`/`consume_gas`/`add_gas`. [`50c015a8`]
- **misc clarity** — Slot::clear dedup, `_hash`→`hash`, state-root comment,
  hsm device_info. [`c2fa97b9`]
- **#10 getstateroot/getstateheight** — fall back to the live MptStore so a
  running node serves real state-root data. [`90a054a3`]
- **#19 NEP-17 balance dedup** (`GasToken::balance_of`, mempool delegates) +
  **#33 neo-tee merkle** routed through `neo_crypto::Crypto::sha256`. [`90a054a3`]
- **#25 unused deps** — dropped async-trait/neo-network/neo-system (consensus),
  uuid/dirs (config), anyhow (tee), bigdecimal (root) + a duplicate lint.
  [`6fcdd74d`]

### Decisions (kept, not changed)
- **#22 neo-network TaskManagerService + typed-wire** — KEPT. Both have an
  integration test / are WIP sync scaffolding; deleting tested intentional WIP
  is a maintainer call, not a cleanup. The 16 conditional-jump opcode handlers
  are likewise kept explicit for 1:1 C# auditability.

### Remaining (precisely scoped follow-ups — each a focused effort)
- **#24 `Result<_,String>` → `CoreResult`/thiserror** across neo-manifest,
  neo-serialization, neo-wallets, neo-system, neo-crypto. Mechanical but broad
  (per-crate enum + call-site sweep); do one crate per change to keep green.
- **#5/#6/#7/#8/#9 node hardening** — DONE: #9 non-blocking broadcast fan-out
  [`4246b022`], #5 TxRouterHandle wired to mempool+broadcast [`6202f3e5`], #6
  mempool total-occupancy eviction [`46b29835`]. REMAINING: #7 bound the in-RAM
  `LedgerContext` with an LRU + durable-store fallback for cold get_block/
  get_block_by_height reads (OOM on mainnet sync); #8 enforce `RpcServerConfig`
  DoS limits + GovernorRateLimiter on the jsonrpsee server (large — jsonrpsee
  0.24 ServerBuilder + a tower middleware layer).
- **#21 neo-vm `VmError`** — trim never-constructed variants + the unused gas
  surface (needs careful per-variant never-constructed verification).
- **neo-hsm `pkcs11` feature** — pre-existing: does not compile (`cryptoki`
  Session is `!Send` under `async_trait`); needs a `spawn_blocking`/Send wrapper.
- **neo-state-service verification pipeline** — now that getstateroot reads
  MptStore, the dormant StateStore/Verifier/commit-handler half can be deleted
  (or wired); structural, deferred.
