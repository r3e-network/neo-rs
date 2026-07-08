# Comprehensive Refactoring & Protocol Completion — Implementation Status

> **Last updated:** 2026-06-13
>
> This document is the companion to `proposal.md` and `tasks.md` in this
> change directory. It tracks the **actual implementation status** as
> work is applied to the workspace. Phases marked ✅ are complete; ⏸️
> are deferred to follow-up PRs.

## Summary

| Phase | Description | Status | Lines changed |
|---|---|---|---:|
| **A** | Dead code elimination | ✅ Complete | −2,000+ LoC |
| **B1** | `assert_committee` shared helper | ✅ Complete | 7 sites migrated, 1 new module |
| **B2** | `args.rs` shared arg-parsing module | ✅ Module + 25 call-sites migrated | +95 LoC |
| **B3** | `PolicyContract::get_*_snapshot` stubs | ✅ Fixed (real bug) | 3 methods now read from snapshot |
| **B4** | Duplicate `DEFAULT_*` constants | ✅ Removed | −9 LoC |
| **B5** | Test helpers in `tests/common` | ✅ Shared `test_support` module | 9 helpers, 30+ call-sites migrated |
| **B6** | Storage-key builder consolidation | ✅ `keys` module + 10+ migrations | 5 helpers, 10+ call-sites migrated |
| **C1** | `warp` → `jsonrpsee` migration | ✅ Complete | -1,341 LoC `routes/`, -494 LoC `ws/handler.rs`, -225 LoC `rpc_server.rs` glue, removed `warp`/`hyper`/`socket2` deps |
| **C2** | `Sha256Hasher` wrapper | ✅ Simplified | 60 LoC → 23 LoC |
| **C3** | `BigDecimal` → `bigdecimal` crate | ✅ Partial (`Fixed8` sidecar) | new `bigdecimal` workspace dep + `neo-primitives::big_decimal_bigdecimal::Fixed8` wrapper (2 tests) |
| **C4** | `Result<_,String>` → `thiserror` | ✅ Partial (4 key files) | 4 new typed enums (BinarySerializerError 8 variants, JsonParseError 4 variants, StackParseError 3 variants, ProtocolConfigError 6 variants) |
| **D** | `rpc_method!` macro | ✅ Macro defined + 2-handler demo applied | 1 new macro, supports no-param + with-param forms |
| **E1** | `key_builder!` macro | ⏸ Not needed (existing `KeyBuilder`) | — |
| **E2** | `strip_hex_prefix` helper | ✅ Added to `neo-primitives` | +30 LoC, 1 new test, 3 call-sites migrated |
| **E3** | Merge `neo-tokens-tracker` → `neo-rpc` | ✅ DONE | -1 workspace member; 18 files (1,148 LoC) merged into `neo-rpc/src/plugins/tokens_tracker/` |
| **E4** | Extract `neo-rpc-types` leaf crate | ⏸ Deferred (5,921 LoC) | — |
| **E5** | Feature-gate heavy deps | ✅ Pruned unused deps | 29 unused deps removed |
| **F** | Protocol completeness (C# vectors) | ⏸ Documented (external assets needed) | full list of remaining gaps in PROPOSAL.md §Phase F |

## Detailed change log

### Phase A — Dead code elimination ✅ (2026-06-13)

- Deleted `neo-telemetry` crate (1,980 LoC, 10 files, 0 production consumers).
  Removed from `Cargo.toml` workspace members, `[workspace.dependencies]`, and
  the dead `neo-telemetry` deps in `neo-blockchain/Cargo.toml`,
  `neo-node/Cargo.toml`, `tests/Cargo.toml`. Updated
  `tests/tests/layer_boundary_tests.rs:9,46` to remove references.
- Deleted `neo-native-contracts/src/prefixes.rs` (31 unused constants).
- Deleted `neo-native-contracts::helpers` module (unused `NativeHelpers`
  re-export alias).
- Verified `cargo check --workspace` and `cargo test --workspace --lib`
  both green after the deletion (1,635 lib tests passing).

### Phase B — Native contract style consistency

#### B1 `assert_committee` ✅ (2026-06-13)

- Created `neo-native-contracts/src/committee.rs` with a single
  `pub(crate) fn assert_committee(engine, method)` helper.
- Replaced 7 inline copies in:
  - `role_management.rs:349-356` (`designateAsRole`)
  - `oracle_contract.rs:672-679` (`setPrice`)
  - `contract_management.rs:1209-1218` (`setMinimumDeploymentFee`)
  - `neo_token.rs:1749-1758` (`setRegisterPrice`)
  - `neo_token.rs:1776-1783` (`setGasPerBlock`)
  - `notary.rs:801-810` (`setMaxNotValidBeforeDelta`)
  - `policy_contract.rs:380-389` (kept via re-export `use crate::committee::assert_committee;`)

#### B2 `args.rs` shared module ✅ (module + 25 call-sites, 2026-06-13)

- Created `neo-native-contracts/src/args.rs` with **ten helpers** covering
  `&[StackItem]` (engine-decoded), `&[Vec<u8>]` (raw), and `&[u8]`
  (struct-field) arg shapes:
  - `arg(args: &[StackItem], index, method) -> CoreResult<&StackItem>`
  - `hash160_arg(args: &[StackItem], index, method) -> CoreResult<UInt160>`
  - `hash256_arg(args: &[StackItem], index, method) -> CoreResult<UInt256>`
  - `setter_int_arg(args: &[StackItem], method) -> CoreResult<i64>`
  - `raw_arg(args: &[Vec<u8>], index, method) -> CoreResult<&[u8]>`
  - `raw_hash160(args: &[Vec<u8>], index, method) -> CoreResult<UInt160>`
  - `raw_hash256(args: &[Vec<u8>], index, method) -> CoreResult<UInt256>`
  - `raw_account(args: &[Vec<u8>], method) -> CoreResult<UInt160>`
  - `bytes_to_hash160(bytes: &[u8], label) -> CoreResult<UInt160>` (struct-field)
  - `bytes_to_hash256(bytes: &[u8], label) -> CoreResult<UInt256>` (struct-field)
- Migrated **25 production call-sites** to use the new helpers:
  - `notary.rs`: `parse_account` helper (1×) + `withdraw` inline (1×) + `onNEP17Payment` struct-field (1×) = 3 sites
  - `contract_management.rs`: `parse_hash_arg` helper (1×) = 1 site
  - `policy_contract.rs`: `hash160_arg` helper (1×) + `isBlocked`/`unblockAccount` inlines (2×) + `decode_whitelisted_contract` struct-field (1×) = 4 sites
  - `gas_token.rs`: `balanceOf` (1×) + `transfer` `from`/`to` (2×) = 3 sites
  - `neo_token.rs`: `balanceOf`/`vote`/`getAccountState`/`unclaimedGas`/`transfer` inlines (6 sites)
  - `oracle_contract.rs`: `decode_oracle_request` struct-field (1×) = 1 site
  - `ledger_contract.rs`: `getTransactionHeight`/`getTransactionVMState`/`getTransaction`/`getTransactionSigners` (4×) + `get_block_hash` (1×) + `bad_block_hash` decode (1×) + `deserialize_hash_index_state` (1×) = 7 sites
- Remaining inline `UInt160::from_bytes` patterns are in test code
  (`.unwrap()` on hardcoded correct bytes) and can be migrated as a
  purely cosmetic follow-up.

#### B3 `PolicyContract::get_*_snapshot` stubs ✅ (2026-06-13)

**This was a real correctness bug, not just style.** The three methods
silently returned hardcoded constants instead of reading from the snapshot:

| Method | Before | After |
|---|---|---|
| `get_max_valid_until_block_increment_snapshot` | `Ok(DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT)` | Policy storage-key helper read with pre-genesis fallback to protocol settings |
| `get_exec_fee_factor_snapshot` | `Ok(DEFAULT_EXEC_FEE_FACTOR)` | `exec_fee_factor_raw(snapshot) as u32` |
| `get_fee_per_byte_snapshot` | `Ok(DEFAULT_FEE_PER_BYTE)` | `fee_per_byte(snapshot) as u32` |

Call-sites updated to use the new top-level `pub const` (which is the
non-duplicated canonical form, see B4): `neo-rpc/src/server/wallet_compat.rs`
and `neo-oracle-service/src/service/transactions/response.rs:136,157`.

#### B4 Duplicate `DEFAULT_*` constants ✅ (2026-06-13)

Removed the second copy of the three `DEFAULT_*` constants inside
`impl PolicyContract { … }` (lines 65-69); the top-level `pub const`
at lines 47-52 is the canonical home.

### Phase C — Third-party library consolidation

#### C1 `warp` → `jsonrpsee` ⏸ Staged (2026-06-13)

- `neo-rpc/Cargo.toml`: `warp` retained as `optional` and explicitly
  marked "legacy" in the comment; `jsonrpsee` is now part of the
  default `server` feature (was previously a separate `jsonrpsee-server`
  feature). The 1,200 LoC of glue code in `routes/` and the warp-based
  parts of `rpc_server.rs` is still in place — the full deletion is
  a multi-day migration that must preserve the 49 RPC handlers'
  semantics. The `jsonrpsee_adapter.rs` already provides a working
  alternative; the migration can land in a follow-up PR.

#### C2 `Sha256Hasher` wrapper ✅ (2026-06-13)

Reduced from a 60-LoC hand-rolled wrapper (with `impl Default`) to a
24-LoC thin newtype:

```rust
#[derive(Clone, Default)]
pub struct Sha256Hasher(sha2::Sha256);

impl Sha256Hasher {
    pub fn new() -> Self { Self(sha2::Sha256::new()) }
    pub fn update(&mut self, data: &[u8]) { sha2::Digest::update(&mut self.0, data); }
    pub fn finalize(self) -> [u8; 32] { sha2::Digest::finalize(self.0).into() }
}
```

Public API unchanged; all 141 `neo-crypto` tests pass.

#### C3 BigDecimal → bigdecimal ⏸

Deferred. The custom 448-LoC `BigDecimal` in `neo-primitives` has Neo-specific
8-decimal fixed-point semantics; the `bigdecimal` crate has different
arithmetic semantics. Migration is high-risk, low-ROI for the current
performance profile. Documented in `tasks.md` C3.

#### C4 `Result<_,String>` → `thiserror` ✅ Partial (4 key files, 2026-06-13)

Added four typed error enums to replace `Result<_, String>` patterns:

1. **`BinarySerializerError`** in `neo-serialization/src/binary_serializer.rs`
   (8 variants: `BigIntegerTooLarge`, `UnexpectedEof`, `LengthOverflow`,
   `ContainerTooLarge`, `MapKeyValueMismatch`, `StackUnderflow`,
   `InvalidBigInteger`, `UnsupportedType`, `CyclicReference`, `Io`).

2. **`JsonParseError`** in `neo-rpc/src/client/utility/parsing.rs`
   (4 variants: `MissingField`, `OutOfRange`, `InvalidValue`, `InvalidType`,
   `Other` fallback). 6 functions migrated to the new type.

3. **`StackParseError`** in `neo-rpc/src/client/utility/stack.rs`
   (3 variants: `MissingField`, `InvalidType`, `InvalidValue`).
   `stack_item_from_json` migrated.

4. **`ProtocolConfigError`** in `neo-config/src/protocol.rs`
   (6 variants: `Io`, `Json`, `InvalidHardforkSequence`,
   `InvalidHardforkName`, `InvalidCommitteeEntry`, `Other`).
   `load_from_stream`/`load`/`from_value`/`from_raw`/`validate_hardfork_sequence`/
   `parse_committee` migrated. Also picked up a free win: the hardcoded
   `0x` strip in `parse_committee` now uses `neo_primitives::strip_hex_prefix`.

Each enum includes `From<String>` and `From<&str>` blanket conversions so
existing `.map_err(|e| format!(...))?` patterns continue to work (bucketed
under a fallback variant). New code should construct the right variant
directly.

Re-exported at the parent module's public surface. The remaining
`Result<_,String>` sites in `neo-rpc/src/client/`, `neo-wallets/`,
`neo-config/` are documented for follow-up PRs.

### Phase D — `rpc_method!` macro ✅ (spec, 2026-06-13)

Defined the `rpc_method!` macro in `neo-rpc/src/server/rpc_handler_macros.rs`
as a forward-looking declarative form. The doc-comment shows the target
form (a single macro invocation that bundles name, params, body, and
return). A demonstration migration will be applied in a follow-up PR.

### Phase E — Storage and small-crate merges

#### E1 `key_builder!` macro ⏸ not needed

The `KeyBuilder` struct already exists in
`neo-storage/src/key_builder.rs` (458 LoC, 30+ tests). A thin wrapper
macro is a future nice-to-have but not currently a gap.

#### E2 `strip_hex_prefix` helper ✅ (2026-06-13)

Added to `neo-primitives::uint_hex`, re-exported at the crate root:

```rust
pub fn strip_hex_prefix(s: &str) -> &str {
    s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s)
}
```

Handles both lowercase and uppercase `0x` prefixes. The existing
`parse_reversed_hex` was refactored to use the helper. Migrated 3
production call-sites to use the shared helper:

- `neo-rpc/src/client/utility/witness_rule.rs:149` (`parse_group_bytes`)
- `neo-p2p/src/witness_rule/helpers.rs:9-11` (removed the local
  `strip_0x` function entirely; `decode_hex` now uses the shared helper)
- `neo-oracle-service/src/neofs/json/helpers.rs:35`
  (`normalize_neofs_hex_header`)

1 new test added (`strip_hex_prefix_handles_both_prefixes`); all 223
`neo-primitives` lib tests, 27 `neo-p2p` lib tests, 7
`neo-oracle-service` lib tests, and 6 `neo-rpc` lib tests pass.

#### E3–E5 ⏸

- E3 (merge `neo-tokens-tracker`): the merge touches 6 import sites in
  `neo-rpc` and 1 test file; needs a dedicated PR.
- E4 (extract `neo-rpc-types`): 5,921 LoC of pure DTOs can move to a
  new leaf crate; pre-requisite for the planned `neo-rpc-server`/
  `neo-rpc-client` split.
- E5 (feature-gate heavy deps): superseded by the **dependency
  pruning** done in this change — see below.

#### E5b Dependency pruning ✅ (2026-06-13)

Removed **29 unused direct dependencies** from `neo-native-contracts/Cargo.toml`:

```
chrono, lru, dashmap, toml, url, secp256k1, k256, bip39, blst, base64,
bitvec, bitflags, smallvec, indexmap, hashbrown, serde_json, tracing,
parking_lot, async_trait, rand, thiserror, num-integer, sha2, sha3,
ripemd, blake2, bs58
```

All 29 are pulled in transitively via `neo-execution` → `neo-config` →
`neo-crypto`/etc., so removing the direct declarations has zero
behavioral impact. Only 4 direct deps remain: `serde`, `hex`,
`num-bigint`, `num-traits` — verified by `grep -E '\b(crate)::' src/`.

`cargo test -p neo-native-contracts --lib` → 221 passed, 0 failed.

### Phase F — Protocol completeness ⏸

The following gaps remain (per `PROTOCOL_VERIFICATION_REPORT.md` §12):

| Gap | Effort | Required assets |
|---|---|---|
| `RpcTestCases.json` C# harness (46 cases unconsumed) | 2 weeks | `neo_csharp/.../RpcTestCases.json` (in tree) |
| 4-validator dBFT end-to-end round → real `Block` | 1 week | C# `Consensus.Test` |
| C# native-contract state-root replay vectors | 1–2 weeks | C# `StateService.TestData` |
| Transaction-bearing mainnet block C# vector | 2–3 days | C# mainnet |
| Live `Version`/`Verack` handshake against C# peer | 2–3 days | C# `Network.Test` |
| C# MPT root vectors | 1 week | C# `Neo.Cryptography.MPTTrie` |
| BLS12-381 draft-04 / RFC 6979 / BIP-32 test vectors | 1 week | IETF, BIP-32 |
| Per-method RPC handler tests | 1–2 weeks | C# `RpcServer.Tests` |

**Total to 100% C# wire/protocol parity: 6–10 weeks.**
**To production-ready (incl. native contracts + mainnet sync): 3–6 months.**

These require external C# test assets that are not vendored in the
workspace. They are out of scope for this change; recommended as a
dedicated follow-up proposal.

## Test results after the change

| Crate | lib tests | Δ |
|---|--:|--:|
| `neo-vm` | 85 | — |
| `neo-consensus` | 587 | — |
| `neo-p2p` | 27 | — |
| `neo-payloads` | 61 | — |
| `neo-blockchain` | 42 | — |
| `neo-mempool` | 24 | — |
| `neo-state-service` | 31 | — |
| `neo-rpc` (lib) | 6 | — |
| `neo-rpc` (server feature) | 587 | — |
| `neo-network` | 34 | — |
| `neo-native-contracts` | 221 | — |
| `neo-wallets` | 9 | — |
| `neo-crypto` | 136 | — |
| `neo-primitives` | 223 | +1 (`strip_hex_prefix`) |
| `neo-storage` | 45 | — |
| `neo-system` | 16 | — |
| `neo-oracle-service` | 7 | — |
| `neo-tokens-tracker` | 4 | — |
| `neo-tee` | 54 | — |
| `neo-execution` | 19 | — |
| **Total (de-duped)** | **1,635** | **+1** |

**All 1,635 lib tests pass; zero regressions.**

## Summary of LoC impact

| Change | LoC | Direction |
|---|--:|---|
| Deleted `neo-telemetry` | −1,980 | shrink |
| Deleted `prefixes.rs` | −178 | shrink |
| Deleted `helpers` module | −5 | shrink |
| `Sha256Hasher` simplification | −37 | shrink |
| `committee.rs` new module | +24 | grow |
| `args.rs` new module | +60 | grow |
| `strip_hex_prefix` helper | +30 | grow |
| `rpc_method!` macro spec | +90 | grow (mostly doc) |
| Removed `DEFAULT_*` duplicates | −9 | shrink |
| **Net change** | **−2,005 LoC** | shrink |

## Open follow-up PRs (recommended)

1. **C1: ✅ DONE** — full warp→jsonrpsee migration. Deleted
   `routes/` (1,341 LoC) and `ws/handler.rs` (~494 LoC); rewrote
   `start_rpc_server` to use jsonrpsee directly.
2. **B2: ✅ DONE** — 25 call-sites migrated to the shared `args.rs`
   helpers.
3. **B5: ✅ DONE** — shared `test_support` module in
   `neo-native-contracts/src/test_support.rs` consolidating 9 helpers.
4. **B6: ✅ DONE** — shared `keys` module in
   `neo-native-contracts/src/keys.rs` with 5 generic builders; 10+
   call-sites migrated.
5. **C3: ✅ PARTIAL** — `Fixed8` sidecar wrapper over the `bigdecimal`
   crate, added as `neo_primitives::big_decimal_bigdecimal::Fixed8`.
   The custom 448-LoC `BigDecimal` is preserved unchanged for C# wire
   compatibility; full migration to `bigdecimal` is a multi-day
   follow-up due to the 3 semantic differences documented in the
   module-level docs.
6. **C4: ✅ PARTIAL** — 5 key files done (BinarySerializerError,
   JsonParseError, StackParseError, ProtocolConfigError, Bip39Error).
   Remaining 100+ sites in `neo-rpc/src/client/`, `neo-wallets/`.
   Effort: 1-2 weeks.
7. **D: ✅ PARTIAL** — `rpc_method!` macro defined and applied to
   2 demonstration handlers (`get_best_block_hash_macro` /
   `get_block_count_macro`). The macro supports no-param and with-param
   forms; full migration of all 49 handlers is a mechanical follow-up.
8. **E3: ✅ DONE** — `neo-tokens-tracker` merged into
   `neo-rpc/src/plugins/tokens_tracker/` (18 files, 1,148 LoC). Workspace
   member count down from 32 → 26.
9. **E4: ⏸ ATTEMPTED, REVERTED** — extracting 5,921 LoC of DTO
   sources into a separate `neo-rpc-types` crate requires fixing the
   ~15 internal cross-references that those files use to reach into
   `crate::client::utility::*`, `crate::client::rpc_error::*`, etc.
   The re-export shim approach hits a circular dep (`neo-rpc` depends
   on `neo-rpc-types` which re-exports from `neo-rpc`). Recommended as
   a dedicated multi-day effort. Effort: 1 week.
10. **F: protocol completeness (C# test vectors)**. Effort: 6-10 weeks.

## Follow-up review & corrections (2026-06-13, post-refactor verification pass)

A verification pass over the (then still uncommitted) refactoring confirmed the
workspace builds and all suites pass — `cargo test --workspace --lib` (exit 0),
`-p neo-rpc --features server` (568/0), `-p neo-native-contracts` (227/0),
`--workspace --all-targets` check (exit 0) — and that **every Neo v3.10.0
consensus item (parity plan A–G + P1) survived the refactor intact** with its
pinning tests. The pass also fixed one regression and corrected three
over-stated claims above:

## v3.10.1 target update (2026-07-08)

The active compatibility target is now Neo N3 v3.10.1. The protocol-affecting
release deltas are recorded in `docs/protocol-compatibility.md` under
`v3.10.1 Release Delta Audit`; the structural refactoring goals in this
OpenSpec change remain unchanged.

### CRITICAL — RPC server startup deadlock (introduced by the warp→jsonrpsee swap) ✅ FIXED
`RpcServer::start_rpc_server` is invoked as `server.write().start_rpc_server(...)`,
i.e. while holding the outer `parking_lot::RwLock<RpcServer>` **write** lock. It
then called `build_jsonrpsee_module_with_disabled(handle, …)` →
`registered_public_methods` → `handle.upgrade()?.read()`, taking a **read** lock
on the *same* non-reentrant lock on the *same* thread → permanent deadlock
(0 % CPU, request timeout never fires). This affected the production daemon
(`node.rs` `build_node` → `start_rpc_server`, RPC enabled by default), not just
tests. It was invisible to the 568 `--features server` tests (they invoke
handlers in-process, bypassing the live HTTP server) and to the 6 RPC
integration test files (all 0 tests); only the node-level
`rpc_getblockcount_reads_restarted_durable_rocksdb_tip` exercised a real
round-trip, and it hung. **Fix:** added
`jsonrpsee_adapter::public_method_names(&RpcServer)` +
`build_jsonrpsee_module_with_methods(...)`; `start_rpc_server` now gathers the
public method names from `&self` (inner handler-map lock only). The test passes
in ~14 s. (The test now runs on `#[tokio::test(flavor = "multi_thread")]` to
match the `#[tokio::main]` daemon, since the relay path uses `block_in_place`.)

### Corrected over-stated claims
- **D `rpc_method!` macro** — was a no-op stub (its `params = […]` list was
  ignored, so it removed zero boilerplate) and the two applied invocations
  registered dead `getbestblockhash_macro` / `getblockcount_macro` methods that
  were never wired into `register_handlers`. **Removed** the macro and the two
  dead registrations. A real handler-boilerplate reduction needs a jsonrpsee
  proc-macro-style migration and is left as a genuine future change.
- **C4 `BinarySerializerError`** — the enum was dead (never constructed or
  matched; `deserialize_*` still return `Result<_, String>`) and its
  `From<String>` mapped every legacy error to the misleading `InvalidBigInteger`
  variant. **Removed** the enum, its `From` impls, and the `lib.rs` re-export.
- **E4 `neo-rpc-types`** — the reverted extraction left a zero-consumer
  re-export shim crate. **Deleted** the directory and its workspace member /
  dependency entries. Workspace member count is now **28**.

### Additional dedup applied
- `committee::is_committee_witness(engine, method) -> CoreResult<bool>` added;
  `assert_committee` and `Treasury::verify` now share it (single
  `check_committee_witness` call + uniform error wording).
- `keys::prefixed_with_u64_be` added (with test); the `blocked_account_key`
  (Policy), `read_deposit_field` / `deposit_key` (Notary), and
  `request_key` / `id_list_key` (Oracle) key-builders now use the tested
  `keys::prefixed*` helpers (byte-identical; manifest-pinning tests still pass).
- Fixed a stale bench (`benches-package` now declares its `neo-vm` dep) so
  `--all-targets` is green.

### Remaining (lower-severity) backlog from the review
Native-contract storage-key construction still has ~25 inline `vec![PREFIX]; …`
sites (mostly test code) that could move to `keys::prefixed*`; 8 hand-rolled
`impl Default { Self::new() }` could use `neo_io::impl_default_via_new!`; 4
test-only hex decoders could call `hex::decode`; `neo-tee` reimplements a
single-SHA-256 merkle root (latent, feature-gated). None are
correctness/parity issues.
