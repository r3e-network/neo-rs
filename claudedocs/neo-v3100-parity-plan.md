# Neo v3.10.0 parity plan (Rust node)

Source: 8-agent gap analysis of C# `v3.9.1...v3.10.0` (39 commits) mapped to neo-rs.
Baseline ref = `neo_csharp/` @ v3.9.1. Gorgon already in Rust `HF_Gorgon` enum (idx 6).

## Reachability key
- C# v3.10.0 `ApplicationEngine.Create` table selection: `Gorgon ? Default : (!Echidna ? NotEchidna : NotGorgon)`.
- Mainnet/testnet: **Echidna scheduled, Gorgon NOT** → current blocks use **NotGorgon** in C# v3.10.0.
- Rust VM uses the single fixed `Default` table → **LIVE divergence** on the NotGorgon-specific ops.

## P0 — consensus-critical

### A. VM jump-table hardfork awareness (BIG; C# semantics VERIFIED 2026-06-11)
- Rust: `neo-execution/src/application_engine/state.rs:36,108` always `ExecutionEngine::new(Some(JumpTable::default()))`; `neo-vm/src/jump_table/*` has only the fixed handlers. `JumpTable` derives `Clone` with `handlers: [Option<InstructionHandler>; 256]` (pub(crate)) — so add `JumpTable::not_gorgon()` / `not_echidna()` constructors IN neo-vm (the field isn't reachable from the host).
- **VERIFIED** against `ApplicationEngine.cs` @ v3.10.0 (full file at /tmp not persisted; re-fetch via `gh api repos/neo-project/neo/contents/src/Neo/SmartContract/ApplicationEngine.cs?ref=v3.10.0`):
  - `ComposeNotGorgonJumpTable()` = `ComposeDefaultJumpTable()` then override `HASKEY=HasKey_Before543, PICKITEM=PickItem_Before543, SETITEM=SetItem_Before543, REMOVE=Remove_Before543, SHR=VulnerableSHR, SHL=VulnerableSHL` (CONFIRMED: NotGorgon DOES restore pre-543 compound + vulnerable shifts).
  - `ComposeNotEchidnaJumpTable()` = `ComposeNotGorgonJumpTable()` then `SUBSTR=VulnerableSubStr`.
  - Selection (`Create`, AE.cs:713): `IsHardforkEnabled(HF_Gorgon, index) ? Default : (!IsHardforkEnabled(HF_Echidna, index) ? NotEchidna : NotGorgon)`.
- **Consensus-continuity note**: mainnet/testnet have Echidna scheduled, Gorgon NOT → current blocks use **NotGorgon** in C# v3.10.0 (and v3.9.1's Default for the Echidna window already had vulnerable shifts since the 567 fix is new). So the Rust node (fixed Default) DIVERGES live on the SHL/SHR shift==0 case. The pre-543 compound deltas are ref-count/exception-class only (lower observability) but still part of the exact table.
- **VulnerableSHL** (and SHR): `shift=Pop().GetInteger(); AssertShift(shift); if (shift==0) return; x=Pop().GetInteger(); Push(x<<shift)` — i.e. on shift==0 it does NOT pop the value operand (the Rust fixed `shift()` in numeric.rs:194 always pops value → faults if non-primitive). Port: pop shift, to_i32, assert_shift, `if shift==0 { return Ok(()) }`, else pop+validate value, push.
- **VulnerableSubStr**: pops count(>=0), index(>=0), x=span; the vulnerable check is `if (index + count > x.Length) throw` (int overflow can wrap → bypass); fixed uses non-overflowing check. Pre-Echidna only.
- **`*_Before543`** (Remove/SetItem/PickItem/HasKey): differ from default in ref-counting order (`RemoveStackReference`/`AddStackReference`) + exception class (`CatchableException` for OOB in SetItem/PickItem vs `InvalidOperationException`). Full C# bodies read 2026-06-11 (AE.cs:316-470). **MUST diff against Rust `neo-vm/src/jump_table/compound.rs` {remove:262, has_key:367, pick_item:589, set_item:639} and the neo-vm reference-counter semantics before porting — highest fork risk.**
- **STATUS: NOT yet implemented (deliberately).** All-or-nothing: a partial NotGorgon table is itself divergent. Needs a dedicated focused implementation + extensive differential tests (esp. shift==0 + non-integer top; ref-count parity). Also includes E below (CheckSig/CheckMultisig Gorgon strict — same `is_hardfork_enabled(Gorgon)` branch in `application_engine_crypto.rs:144 verify_signature`: post-Gorgon a bad sig length / invalid pubkey FAULTS; pre-Gorgon returns false; note the pre-existing Rust pre-Gorgon-faults-on-invalid-pubkey divergence to fix to lenient).

### B. Native method/event activation predicate (#4520/#4524) — small, clear
- Rust: `neo-execution/src/native_contract.rs:596-605 is_active_for` — OR-form bug.
- Fix → AND-form: `(active_in.is_none() || hf(active_in)) && (deprecated_in.is_none() || !hf(deprecated_in))`.

### C. Conflicts duplicate-attribute rejection — small, clear, LIVE
- Rust: `neo-mempool/src/verification.rs:384` only does per-attr on-chain check.
- Fix: reject (InvalidAttribute) if tx has duplicate Conflicts hashes (`count != distinct count`).

### D. RoleManagement designateAsRole duplicate-pubkey rejection — small, clear, LIVE
- Rust: `neo-native-contracts/src/role_management.rs:315-388`.
- Fix: reject when node list has duplicate ECPoints (`nodes.len() != dedup.len()`).

### E. CryptoLib Gorgon strict verify + CheckSig/CheckMultisig (Gorgon-DORMANT)
- verifyWithECDsa V2 (active_in Gorgon) + verifyWithEd25519 V1 (active_in Gorgon): malformed sig-len/pubkey FAULTS (not Ok(false)). Pre-Gorgon V1/V0 keep lenient.
- Rust: `neo-native-contracts/src/crypto_lib.rs` (dual→triple/dual registration + strict path) and `neo-execution/src/application_engine_crypto.rs:144 verify_signature` (Gorgon branch: fault on bad len/pubkey).

### F. ContractManagement destroy V0/V1 split (Gorgon-DORMANT)
- Rust: `contract_management.rs:881` single destroy; `:1290` delete-then-block order.
- Fix: register destroy V0 (deprecated_in Gorgon) + V1 (active_in Gorgon); V1 does block_account+clean_whitelist BEFORE erase.

### G. Policy recoverFund callflags States|AllowNotify → All (adds AllowCall) — DORMANT(HF_Faun)
- Rust: `policy_contract.rs:1051` + assertion `:1838`.

## P1 — protocol
- **VerifyResult.NotYetValid** enum: insert after Expired (byte 11), renumber InsufficientFunds=12/PolicyFail=13/HasConflicts=14/Unknown=15. `neo-primitives/src/verify_result.rs`. Update RPC relay match + roundtrip test.
- **Transaction.VerifyStateDependent ValidUntilBlock split** (`verification.rs:258`): `<= height` → Expired; `> height + maxIncrement` → NotYetValid (boundary unchanged, classification only).
- **Notary SetMaxNotValidBeforeDelta** lower bound 0 → `engine.protocol_settings().validators_count` (`notary.rs:768`).
- pre-543 compound variants (part of A).

## P3 — none (non-consensus, skip)
- All `IReferenceCounter` parameter removals (ToStackItem/BinarySerializer/JsonSerializer/StorageIterator/StorageItem/Runtime) — internal refactor, identical stack items.
- Iterator.Value() ref-counter drop; ECDsaCache IDisposable; FeeConsumed/GasLeft overflow guards (Rust i64 saturating already safe); Oracle/Notary exception-message text; Diagnostic.CallFromNative hook (observational).

## Implementation order + status
1. **Batch-1 DONE + committed (269490dd)**: B (is_active_for AND-form), C (conflicts dedup), D (role dedup),
   Notary-bound, VerifyResult.NotYetValid + ValidUntilBlock split + RPC relay, G (recoverFund callflags). Green.
2. **Batch-2 DONE + committed (b5152973)**: F (ContractManagement destroy V0/V1 split) + CryptoLib V2/Ed25519-V1
   registrations + strict dispatch. Green. (E's CheckSig/CheckMultisig interop moved to batch-3.)
3. **Batch-3 DONE + committed (28dfe84b)**: A jump-table hardfork awareness + E CheckSig/CheckMultisig Gorgon
   gating. `ApplicationEngine::select_jump_table` picks default/not_gorgon/not_echidna by persisting-block
   hardforks; neo-vm `JumpTable::not_gorgon()`/`not_echidna()` with vulnerable SHL/SHR + pre-543 compound;
   CheckSig/CheckMultisig gate `VerifySignatureV0` (pre-Gorgon, wrong-len sig → false) vs strict
   `VerifySignature` (from Gorgon → fault), pubkey decoded before the sig-length check. **CORRECTION to the
   earlier plan**: the C# pre/post-Gorgon CheckSig difference is ONLY the signature-length handling — the
   public key is `DecodePoint`-decoded (faults on invalid) in BOTH, so the Rust pubkey-faults behaviour was
   already correct. SUBSTR vulnerable is a consensus no-op (documented). Tests + full gate green.

**v3.10.0 alignment COMPLETE: all P0/P1 consensus items landed.** P3-none items (IReferenceCounter removals,
exception-message text, FeeConsumed/GasLeft guards, Diagnostic.CallFromNative) intentionally skipped as
non-consensus.

Gate every batch: `cargo test --workspace` + `-p neo-rpc --features server` + `-p neo-node --features wip`.
No AI attribution in commits.
