# neo-rs Crate-Boundary Audit — 2026-06-08

Parity target: C# Neo N3 **v3.9.1 / v3.9.2** (reference checkout in `neo_csharp/`).
Method: full-workspace dependency-graph extraction + per-cluster source reads +
real `cargo` exit-code verification (not `| tail`, which masks cargo's status).

> **This doc reflects the post-`kill-neo-core` reality.** The monolithic
> `neo-core` crate is **gone**; it was dissolved into 48 crates. Earlier docs
> (`crate-boundary-refactor-plan.md`, `neo-core-dissolution-validated-dag.md`)
> describe the *in-progress* dissolution and are now historical. Use this file
> as the current map.

## 1. Structural state — the dissolution succeeded

- **48 library crates** (was a ~100K-LOC `neo-core` monolith).
- **Dependency graph is fully acyclic** (verified via SCC analysis). No cycles.
- Clean 17-layer DAG, `neo-primitives` (L0, fan-in 37) → `neo-node` (L16).
- Largest crates: `neo-rpc` 36.5K, `neo-vm` 12.8K, `neo-execution` 10.3K,
  `neo-consensus` 7.6K, `neo-node` 7.4K, `neo-primitives` 7.1K, `neo-crypto` 6.3K.

This is a genuine achievement and matches reth/polkadot practice (no monolithic
core, types vs traits vs impls split). The remaining work is **completeness,
overlap elimination, and parity**, not further decomposition.

## 2. Build baseline reality (corrected)

| Build | Status (start of session) | After session fixes |
|-------|---------------------------|----------------------|
| `cargo check --workspace --all-targets` (default) | **RED** (exit 101) | **GREEN** ✅ |
| `cargo check --workspace --all-targets --all-features` | **RED** (41 errors) | **RED** (neo-rpc features) |

The default build was broken by misplaced crate-level `#![doc(html_root_url)]`
inner attributes in two submodules (`invalid_doc_attributes` is deny-by-default)
and a stale bench import — **now fixed** (commit `88857107`). Prior "green"
readings in earlier sessions were almost certainly `tail`'s exit code, not
cargo's; always verify with `cargo check ...; echo $?` or `${PIPESTATUS[0]}`.

## 3. Findings (prioritized)

### P1 — `neo-rpc` feature-gated code does not compile (biggest completeness gap)

`neo-rpc` builds green only because `default = []` builds it nearly empty. Its
real features are broken:
- `--features server`: **41 errors**
- `--features client`: **72 errors**

This is core node functionality (the JSON-RPC server + client SDK) that rotted
during the dissolution and was never migrated. Root causes:

1. **`NeoSystem` was deleted/renamed → `neo_system::Node`** (reth-style), but the
   RPC server still holds `Arc<NeoSystem>` and calls `NeoSystem::new(...)`
   (`neo-rpc/src/server/rpc_server.rs:101,125`, `session.rs:95`,
   `rpc_server_utilities.rs:172+`). This is a **real migration**, not a repoint:
   every `system.<method>()` call site must move to the new `Node` API.
2. **`AssetDescriptor` was removed** (`rpc_server_wallet/mod.rs:20` literally says
   `// AssetDescriptor removed;`) yet is still used (`:332,:457,:724`). C# **has**
   `Neo.Wallets.AssetDescriptor` (`neo_csharp/src/Neo/Wallets/AssetDescriptor.cs`)
   → genuine **parity gap**: re-port `AssetDescriptor` into `neo-wallets`
   (queries NEP-17 `symbol`/`decimals` from a contract).
3. **`STATE_STORE_SERVICE`** const was moved (`rpc_server_state.rs:10`
   `// STATE_STORE_SERVICE moved;`) — redefine/repoint.
4. **Mechanical stale imports** (the bulk — repoint to canonical homes):
   - `neo_crypto::KeyPair` → `neo_wallets::KeyPair` (×5)
   - `neo_block::Block` / `BlockHeader` → `neo_payloads::{Block, BlockHeader}`
   - `neo_payloads::header::BlockHeader` → `neo_payloads::BlockHeader`
   - `neo_smart_contract_types::MethodToken` → `neo_manifest::MethodToken` ✅ done
   - `neo_primitives::VerifiableExt` → `neo_payloads::VerifiableExt`
   - `ContractParametersContext` → `neo_execution::ContractParametersContext`
   - `ContractParameterType` → `neo_primitives::ContractParameterType`
   - `CallFlags` → `neo_primitives::CallFlags`
   - `Wallet` → `neo_wallets::Wallet`; `RpcService` → `neo_services::RpcService`
   - `LocalNode` → `neo_network::LocalNodeService` (renamed)
   - `StateRoot`/`StateStore` → `neo_state_service::*`
   - `get_sign_data`/`get_sign_data_vec` → `neo_payloads::*`
   - `oracle_response_code` → `neo_primitives::oracle_response_code`

Recommended sequencing: (a) mechanical repoints first (shrinks error count, no
judgment); (b) re-port `AssetDescriptor` into `neo-wallets` with C# parity;
(c) the `NeoSystem → Node` migration as a focused unit; (d) **then** extract
`neo-rpc-types` (~5.7K LOC of pure `client/models/*` structs) into its own crate
used by both client and server — this finally breaks up the 36.5K monolith.
**No stubs / no commenting-out** — if a symbol is genuinely gone, port it.

### P1 — `neo-node` does not wire RPC or consensus

`neo-node`'s `wip` (a.k.a. `full`) feature only pulls
`neo-config/blockchain/storage/system/block/primitives/event-handlers`. It does
**not** enable `neo-rpc`, `neo-consensus`, `neo-network` (directly), `neo-tee`,
`neo-hsm` by default. A node that can't serve RPC or reach consensus is
incomplete. Blocked on P1 (neo-rpc must compile first). Track wiring + an
integration smoke test once RPC compiles.

### P2 — True type duplication: `Header` / `HeadersPayload`

Independently **defined twice**:
- `neo-payloads/src/header.rs:27` (canonical — used by system/rpc/node/execution/blockchain)
- `neo-ledger-types/src/header/mod.rs:23` (**zero importers** — dead)
- `HeadersPayload`: `neo-payloads/src/headers_payload.rs:21` vs
  `neo-ledger-types/src/headers_payload.rs:41`

`Header` is consensus-critical serialization (block header byte layout → hash →
genesis KAT). Consolidation must be byte-identity-verified against C#
(`neo_csharp/.../Payloads/Header.cs`). Subtlety: confirm the exact dep direction
between `neo-p2p`, `neo-ledger-types`, and `neo-payloads` before moving — the
layering around the `HeadersPayload` consumer needs care. **Investigate then
collapse onto `neo-payloads`; keep `neo-ledger-types::Witness`** (that one is the
single, widely-used canonical Witness home — not a dup).

### P2 — `neo-error` god-error inverts Layer 0

`neo-error` (the `CoreError` authority, fan-in 20) sits at L4 because it carries
5 cross-crate `From<X>` impls (`neo-error/src/error.rs:487-525`):
`StorageError`, `KeyBuilderError`, `VmError`, `ScriptBuilderError`,
`RedeemScriptError`. So 20 crates depending on `neo-error` transitively pull in
`neo-vm`/`neo-storage`/`neo-script-builder`. There is an
**in-code TODO** (`error.rs:482-485`) to invert this: have each crate provide its
own `From<LocalError> for neo_error::CoreError` and trim `neo-error`'s deps to
`neo-primitives + neo-io`. Doing so realigns with reth's per-crate-error model.
Low risk (additive `From` impls), high tidiness payoff.

### P3 — Thin re-export facades (overlap-by-aliasing)

- **`neo-smart-contract-types`** — pure facade of `neo-manifest`. **REMOVED ✅**
  (commit `f6971cf5`; was also a duplicate workspace member).
- **`neo-storage`** (119 LOC) — re-exports `StorageKey`/`StorageItem`/
  `DataCache` from `neo-storage` (the canonical definitions). Unlike the above,
  it has **~8 crate dependents**, so elimination = repoint those to
  `neo_storage::*` (medium mechanical effort). Decide: keep as deliberate compat
  alias, or fold into `neo-storage` exports.

### P3 — Stale `neo-core` references in docs (76 files, 172 occurrences)

`neo-core` is deleted but module docs across the tree still say "lives in
neo-core" / "lifted out of neo-core" / "neo-core's BlockVerificationExt". **All
are comments** (zero non-comment `neo_core::` code refs; 6 Cargo.toml refs are
comments too) — so this is a doc-correctness sweep, not a build issue. Watch for
intra-doc links (`[`neo_core::X`]`) that would warn under `cargo doc`.

## 4. Verified NON-issues (do not "fix")

- `VerifyResult` (neo-block + neo-p2p) and `InventoryType` (neo-p2p) are **thin
  re-exports of the single `neo_primitives` source of truth** — not dups.
- `neo-storage` / `neo-storage` / `neo-serialization` / `neo-storage-rocksdb`
  have clean, non-overlapping responsibilities (types / facade / codecs+compression
  / backend). No duplicate definitions.
- `neo-execution` vs `neo-native-contracts` vs `neo-manifest`: clean engine /
  contract-handles / wire-types split. (Verify native-contract completeness vs
  C# separately — see §5.)
- `neo-io` low-level compression vs `neo-serialization` `CoreError`-wrapped +
  Zstd: proper layering, not dup.

## 5. Parity follow-ups (C# v3.9.1/2)

- `AssetDescriptor` missing (see P1.2).
- Audit the native-contract surface in `neo-native-contracts` against
  `neo_csharp/src/Neo/SmartContract/Native/*` — confirm each contract's methods
  are present, not just handle accessors. (`claudedocs/audit-findings.md` lists
  prior parity findings; re-verify against current tree.)
- Confirm `neo-node` exposes the full RPC method set C# `RpcServer` does once P1
  lands.

## 6. Fixed this session

1. `88857107` — restore default-features compilation (misplaced doc attrs + bench import).
2. `3d638379` — gitignore local C# reference checkouts (`neo_csharp/`, `neo_csharp_vm/`).
3. `f6971cf5` — remove dead `neo-smart-contract-types` facade + duplicate member.
4. `5d0a98e9` — repair malformed `use` in `neo-rpc` blockchain handler.

## 7. Roadmap (suggested order — each step ends green + committed)

1. **Stale-doc sweep** (P3) — low-risk, mechanical, improves every crate's header.
2. **`neo-error` inversion** (P2) — additive `From` impls per crate; trim deps.
3. **`Header`/`HeadersPayload` consolidation** (P2) — byte-identity verified.
4. **`neo-rpc` mechanical repoints** (P1.4) — shrink the error count.
5. **`AssetDescriptor` re-port** to `neo-wallets` (P1.2 / parity).
6. **`NeoSystem → Node` migration** in neo-rpc (P1.1) — focused unit.
7. **Extract `neo-rpc-types`** (P1) — break the 36.5K monolith.
8. **Wire RPC + consensus into `neo-node`** (P1) + integration smoke test.
9. **`neo-storage` decision** (P3).
10. **Native-contract parity pass** vs C# (§5).

Consensus guard for every step touching ledger/payloads/native code: genesis
block-hash KAT + block/transaction serialization round-trips must not drift.
