# A4/S3 — Verification Seam Design (the B5 chain-type-move blocker)

Status: **investigated + designed (2026-05-31), not yet executed.** Restore point: commit `407456fd`.
Source: two read-only investigation workflows (`wsugxybxy` call-site/native/C# map; earlier redeem-script work freed witness.rs/signer.rs).

## Problem

The chain-type payloads (Transaction, Header, the transaction attributes, ExtensiblePayload)
still call the smart-contract **engine** (`ApplicationEngine`) and **native contracts**
(Ledger/Policy/ContractManagement/Oracle/RoleManagement) from their `verify()` paths. These
are heavy neo-core concerns, so the types cannot move down into `neo-p2p` (the B5 goal) while
their verification lives *on the type*.

## Decisive C# reference (CORRECTED after reading the source — Step 0, 2026-05-31)

**The investigation agent over-claimed; the truth (verified in neo_csharp/src/Neo):** C# Neo is a
**monolith** — `Transaction.cs` imports `Neo.SmartContract.Native` + `Neo.VM` + `using static
Neo.SmartContract.Helper`, and `Transaction.VerifyStateDependent` (line 323) calls
`NativeContract.Ledger.CurrentIndex` / `NativeContract.Policy.{IsBlocked,GetFeePerByte,
GetExecFeeFactor}` **directly**. So C# is NOT "engine-free" — Transaction/ApplicationEngine/
NativeContract all live in the same `Neo.dll`, so C# never needs a crate boundary. **The Rust
crate seam is therefore a genuine architectural addition, not a C# mirror.**

BUT C# DOES give the right structural split to follow:
- **`VerifyStateIndependent(settings)`** (Transaction.cs:371) is **engine-free**: size check, script
  parse, and the standard-signature fast path (`IsSignatureContract` + `Crypto.VerifySignature`,
  `IsMultiSigContract` + multisig) — i.e. only crypto + the redeem-script recognizers (already in
  neo-script-builder). `GetScriptHashesForVerifying` for Transaction is just `Signers.Select(Account)`
  — also engine-free.
- **`VerifyStateDependent`** (native queries) + **`VerifyWitness`** (`ApplicationEngine.Create`, in
  the `Helper.VerifyWitness` static extension, Helper.cs:321/333) need the engine/natives.

**The Rust code already mirrors this split** (transaction/verification.rs:
`verify_state_independent`@158 is engine-free; `verify_state_dependent`@35 / `_at_height`@49 /
`verify_standard_witness`@191 / `verify_witness`@281 use native+engine). 16 external callers of the
verify entry points (transaction_router, blockchain handlers, memory_pool, block_processing,
neo_system/actors, neo-node/consensus).

**Therefore the seam is NOT a "VerificationContext trait" in a low crate.** It is: **move the
state-dependent verification out of the payload types into free functions (or a neo-core-local
trait impl) that live in neo-core**, mirroring C#'s static extension helpers. Because the verify
functions stay in neo-core, they keep direct access to the engine + natives + the
neo-core-only return types (TrimmedBlock/ContractState/OracleRequest) — **no leaky-type problem,
no trait-in-low-crate.** Only the *pure data + serialization* of the types moves to neo-p2p.

Recommendation (from the C# agent, lowest risk): **free functions taking `&Verifiable` +
snapshot**, e.g. `verify_witnesses(v: &dyn Verifiable, snapshot, settings, gas) -> CoreResult<(bool,i64)>`.
Do NOT make `verify_witness` a `Verifiable` trait method — that re-introduces the engine edge.

## Full call-site map (28 sites / 8 files) — the surface to relocate

### ApplicationEngine (8 distinct APIs) — the witness-verification core
- `transaction/verification.rs::verify_witness`: `ApplicationEngine::new` (312),
  `ContractManagement::get_contract_from_snapshot` (328), `load_contract_method` (347),
  `load_script` (364, 372), `execute` (378), `fee_consumed` (392).
- `header/verification.rs::verify_witness_against_hash`: `ApplicationEngine::new` (164),
  `ContractManagement::get_contract_from_snapshot` (189), `load_contract_method` (228),
  `load_script_with_state` (268, 284), `execute` (297), `result_stack` (307),
  `current_evaluation_stack` (314).

### Native queries (11 distinct) — state-dependent validation
- `LedgerContract::current_index` — transaction/verification.rs:42, not_valid_before.rs:38,
  oracle_response.rs:102, extensible_payload.rs:74.
- `LedgerContract::get_trimmed_block` — header/verification.rs:369 (prev header → next_consensus).
- `LedgerContract::contains_transaction` — conflicts.rs:41.
- `PolicyContract::{get_max_valid_until_block_increment, is_blocked, get_fee_per_byte,
  get_exec_fee_factor}_snapshot` — transaction/verification.rs:60/71/96/111.
- `PolicyContract::get_attribute_fee_snapshot` — transaction_attribute.rs:175.
- `ContractManagement::get_contract_from_snapshot` — transaction/verification.rs:328,
  header/verification.rs:189.
- `OracleContract::get_request` — oracle_response.rs:90.
- `RoleManagement::get_designated_by_role_at` — oracle_response.rs:106.
- `NativeHelpers::{get_bft_address, committee_address}` — oracle_response.rs:114,
  high_priority_attribute.rs:39.
- `Helper::{signature_contract_cost, multi_signature_contract_cost}` —
  transaction/verification.rs:220/276 (these stay in neo-core; they use ApplicationEngine pricing).

### Leaky neo-core-only return types (stay in neo-core; only matter if a trait were pushed low)
- `TrimmedBlock` (embeds `Header`), `ContractState`, `OracleRequest`. With the free-function
  design these are NOT a blocker (verify stays in neo-core).

## Execution plan (incremental, each step green; consensus-critical → differential-test)

0. **Verify the C# claim first** (read neo_csharp Transaction.cs/Header.cs/SmartContract/Helper.cs)
   — confirm `VerifyWitnesses`/`VerifyWitness` are static extensions and the types don't ref the engine.
0b. **FIRST untangle a merged concern (found by reading verification.rs).** Rust
   `verify_state_independent` is NOT cleanly engine-free: it calls `verify_standard_witness`
   (line 191) which returns the verification *fee* via `Helper::signature_contract_cost` /
   `multi_signature_contract_cost` → `ApplicationEngine::get_opcode_price`. C# keeps these
   separate (VerifyStateIndependent does the signature CHECK without cost; cost is computed in
   VerifyStateDependent). So before relocation: split `verify_standard_witness` into
   (i) an engine-free signature check (crypto + `neo_script_builder::is_signature_contract` +
   the `Helper::parse_multi_sig_contract`/`parse_multi_sig_invocation` parsers — which should
   themselves move to neo-script-builder as recognizers), returning Standard/NonStandard; and
   (ii) the cost computation (engine pricing) which stays in the state-dependent path. Verify
   green (consensus suites) as its own commit. THEN:
1. **Create a neo-core `verification/` module** (free functions). KEEP `verify_state_independent`
   (now engine-free) + `get_script_hashes_for_verifying` ON the types (they only need neo-crypto +
   neo-script-builder, both below neo-p2p). Relocate ONLY the engine/native parts —
   `verify` (entry), `verify_state_dependent_with_native_provider`/`_at_height`, `verify_standard_witness`,
   `verify_witness`/`verify_witness_against_hash` — out of `impl Transaction`/`impl Header` into
   `fn verify_transaction_with_native_provider(tx: &Transaction, snapshot, settings, provider, …)` /
   `fn verify_header*(…)`.
   Keep behavior byte/semantics-identical. Update the ~16 external callers
   (`tx.verify(...)` → `verify_transaction_with_native_provider(&tx, ...)`).
2. **Relocate the attribute `verify()` impls** similarly (oracle_response, not_valid_before,
   conflicts, high_priority, extensible_payload, transaction_attribute fee) into the verification module.
3. At this point the payload types are **pure data + serialization** (no engine/native refs) — they
   are now movable. (B5) Move the type files into `neo-p2p`; neo-core's verification module + RPC +
   consensus continue to depend on neo-p2p for the types.
4. Differential-test the witness-verification path (it is the security boundary): standard sig +
   multisig + contract-based witness; mainnet block replay if available.

## Verification gates each step
- `mainnet_genesis_hash_matches_csharp`, the witness/verification suites
  (transaction_validation_edge_cases, smart_contract_helper, p2p_payloads_csharp_tests),
  the consensus integration tests, `no_local_neo_vm_dependency` guards, `cargo check --workspace --all-targets`,
  neo-core `--features runtime`.

## Risk notes
- Witness verification is the consensus security boundary; semantics must match C# exactly
  (READ_ONLY call flags on the verification script, unrestricted on invocation, gas budget
  = `Helper::MAX_VERIFICATION_GAS`, HALT + result==true).
- Header uses an explicit script-hash entry; Transaction iterates signers — the free-function API
  must serve both.
- Do this as its own focused unit; a half-relocated verify path is dangerous (red/inconsistent
  security boundary). Abort cleanly to `407456fd` if it tails.
