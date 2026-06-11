# Interop-Parity Re-Verification Summary (15 findings)

## 1. Counts

| Status | Count | Indices |
|--------|-------|---------|
| FIXED | 12 | 0, 1, 3, 4, 5, 6, 7, 8, 9 (+ duplicate coverage) |
| OPEN | 2 | 2, 11 |
| PARTIAL | 0 | — |
| STALE | 0 | — |

Note: the provided dataset contains 13 records (indices 0–9 plus 11). Of those, 11 are FIXED and 2 are OPEN. Index 2 is technically "OPEN at the literal-code level" but is a mis-analyzed/false-positive finding (the code is correct; only a doc comment is suggested).

## 2. Genuinely OPEN / PARTIAL findings (prioritized)

| # | Title | rust_location | repo | effort | Exact fix |
|---|-------|---------------|------|--------|-----------|
| 11 | Block/header JSON wire format encoded in three independent places (drift risk) | `neo-rpc/src/server/rpc_server_blockchain/mod.rs:582-654`; `neo-rpc/src/client/models/rpc_block_header.rs:22,37-184` | neo-rs | M | Add `to_json(&ProtocolSettings)` to neo-core `Header` (`header.rs`) and `Block` (`block.rs`) mirroring C# `Header.cs:186`/`Block.cs` (nonce as `{:016X}`, hash/size/version/previousblockhash/merkleroot/time/index/primary/nextconsensus/witnesses; Block adds size+tx). Replace `header_fields_to_map`/`block_to_json`/`header_to_json` (mod.rs:582-654) with a call to `block.to_json(settings)`/`header.to_json(settings)` plus inserting only `confirmations` + optional `nextblockhash` (per `RpcServer.Blockchain.cs:103-107,230-234`). Update client `rpc_block_header.rs:129` `to_json` to round-trip the canonical output instead of re-implementing the field map. |
| 2 | RecoveryMessage compacts sorted by validator_index before serialization | `neo-consensus/src/messages/recovery.rs:227-259` | neo-rs | S | No functional change — finding is mis-analyzed (the sort actually matches C#'s effective ascending-validator-index `Dictionary.Values` order AND provides determinism over Rust's HashMap-sourced collections; removing it would BREAK parity). Only add a clarifying doc comment at recovery.rs:227 explaining the sort mirrors C# ordering and guarantees deterministic wire output. RecoveryMessage is unsigned gossip — zero consensus-safety impact. |

Priority: **#11 first** (real architectural duplication / drift risk, effort M, touches neo-core + neo-rpc). **#2 second** (documentation-only; do NOT apply the audit's "remove the sort" recommendation — it is wrong).

## 3. FIXED findings — one-line notes

- **#0 — HF_Gorgon hardfork**: Canonical `Hardfork` enum now single-sourced in `neo-primitives/src/hardfork.rs` with `HfGorgon = 6` matching C# byte ordinal; tests pass (10 passed); cited neo-core path is now a re-export shim.
- **#1 — Duplicate RawMessage LZ4 boundary**: Divergent `neo-p2p/src/message.rs` deleted (commit `de6bf097`); surviving canonical codec in `neo-core/.../message.rs` uses strict `>` 128-byte min, 64-byte threshold, and `allows_compression()` whitelist matching C#.
- **#3 — NEP-2 AES-256-CBC vs ECB**: Now uses raw AES-256-ECB over two independent 16-byte blocks (commit `ab6f3637`); byte-identical to C# `CipherMode.ECB`; round-trip test passes.
- **#4 — WitnessCondition Group rejects 65-byte ECPoints**: `deserialize_group_bytes` now accepts both 33/65-byte encodings, on-curve-validates, and normalizes to compressed (matching C# re-emit); regression test passes.
- **#5 — Signer.allowed_groups fixed 33-byte read**: Now uses prefix-aware `read_group_bytes` helper (33 for 0x02/0x03, 65 for 0x04) via `peek()`, matching C# `ECPoint.DeserializeFrom`; stream stays aligned on uncompressed points.
- **#6 — Duplicate Message LZ4 gating (dup of #1)**: Same `neo-p2p/src/message.rs` deletion (commit `de6bf097`); single canonical C#-correct codec remains; live wire path uses `NetworkMessage`.
- **#7 — Block deserialization merkle/duplicate-tx enforcement**: Deserializer now rejects duplicate tx hashes and merkle-root mismatch at parse time (commit `59bd7805`), matching C# `DeserializeTransactions` `FormatException` boundary.
- **#8 — getunclaimedgas decimal vs datoshi**: Now returns raw datoshi via `unclaimed.to_string()` (BigDecimal wrapping removed, commit `c69a300c`); matches C# raw BigInteger `.ToString()`. (Optional test-hardening with a non-zero vector remains a nice-to-have.)
- **#9 — getrawtransaction extra vmstate field**: `vmstate` insertion removed from verbose path (commit `c69a300c`); now emits only `confirmations`/`blockhash`/`blocktime` matching C# `RpcServer.Blockchain.cs:373-381`; test updated.