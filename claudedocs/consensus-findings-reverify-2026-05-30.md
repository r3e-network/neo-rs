# Neo Consensus-Parity Re-Verification (24 findings)

## 1. Headline Counts

| Status | Count | Indices |
|--------|-------|---------|
| **FIXED** | 18 | 0,1,2,3,4,5,10,11,12,14,15,16,17,18,20,21,22,23 |
| **OPEN** | 4 | 6,7,8,13 |
| **PARTIAL** | 2 | 9,19 |
| **STALE** | 0 | — |

All 4 OPEN findings and the still-open half of finding #9 share one root cause: the **neo-vm-rs external-VM fast path** routed onto the consensus path via `load_execute_storage.rs:93-95`. Finding #13 is the structural parent; #6, #7, #8, and #9(external) are specific limit divergences on that same path.

## 2. Genuinely OPEN Findings (prioritized: consensus_risk, then effort)

| Idx | Title | rust_location | risk | effort | Exact fix |
|-----|-------|---------------|------|--------|-----------|
| **6** | External VM caps NEWBUFFER/CAT item size at 1MB instead of 131070 | neo-vm-rs `semantics/collections.rs:98`, `semantics/splice.rs:14`, const `vm/limits.rs:15` | breaks-parity | **M** | Thread the engine limit into semantics: change `new_buffer(size)`→`new_buffer(size, max_item_size)` and `cat_values`→accept limit, compare `> max_item_size` (131070); pass `limits.max_item_size as usize` from `byte_ops.rs:27,38`. Quick fallback: set const `MAX_ITEM_SIZE = (u16::MAX as usize)*2` (keep `MAX_SCRIPT_SIZE`=1MB). |
| **9 (external half)** | MaxComparableSize not enforced by external-VM EQUAL/NOTEQUAL | neo-vm-rs `helpers/values.rs:70-95`, `runtime_types.rs:207-233`, `executor/numeric_ops.rs:33-42` | breaks-parity | **M** | Thread `ExecutionEngineLimits` into EQUAL/NOTEQUAL dispatch (`numeric_ops.rs:33-42`); replace `vm_equal()->bool` with fallible `equals_with_limits()->Result<bool,String>` mirroring neo-core's `byte_string_size_eq_with_budget` + `struct_equals_with_limits` (ByteString fault `size>limits||limits==0`; Struct iterative two-stack walk bounded by `max_stack_size` + decrementing `max_comparable_size`). Return Err→FAULT. (neo-core primary path already FIXED.) |
| **8** | External VM caps call depth at 64 instead of MaxInvocationStackSize=1024 | neo-vm-rs `interpreter/state.rs:110`, `state/call_stack.rs:14,75` | breaks-parity | **M** | Raise `MAX_CALL_DEPTH` 64→1024 (matches existing `DEFAULT_MAX_INVOCATION_DEPTH`). Convert `frames: [MaybeUninit<CallFrame>; MAX_CALL_DEPTH]` to a growable `Vec<CallFrame>` on the native (`not(riscv32)`) path; keep the `if self.len >= MAX_CALL_DEPTH` guard so FAULT lands exactly at 1024. Add CALL/CALLA differential tests at depths 64/65/1023/1024. |
| **7** | External VM enforces MaxStackSize as eval-stack length, not C# reference count | neo-vm-rs `interpreter/executor/mod.rs:145`, `interpreter/state.rs:108` | breaks-parity | **L** | Preferred: stop routing consensus through external VM (see #13) so neo-core's ReferenceCounter engine is authoritative. Otherwise implement C#-equivalent reference counting (eval stack + slots + nested compound sub-items via AddReference/RemoveReference + CheckZeroReferred SCC cleanup) enforced after each instruction; thread through PACK/APPEND/NEWARRAY/SETITEM/slot stores. |
| **13** | Two parallel VM engines on the consensus path (root cause of 6/7/8/9-ext) | neo-core `application_engine/load_execute_storage.rs:93`, `external_vm.rs:254`; neo-core `neo_vm/execution_engine/execution.rs:10`; neo-vm-rs `interpreter/api.rs:66` | high | **L** | (1) Add `fuzz/fuzz_targets/fuzz_vm_differential.rs` asserting identical (VMState, result-stack, fault) between external and local engines across edge sizes/depths. (2) Until green in CI, gate the fast path behind `#[cfg(feature="external_vm_fastpath")]` (OFF default) — consensus runs only the local jump_table engine; update `tests/tests/no_local_neo_vm_dependency.rs`. (3) Long term: make neo-core jump_table the single consensus engine, confine neo-vm-rs to zk/TEE, delete the fast path. **Fixing #13 via gating resolves 6/7/8/9-ext at once.**

**Recommended sequencing:** Do **#13 step 2 (feature-gate the fast path OFF)** first — it is the single cheapest change that closes all four breaks-parity divergences (#6, #7, #8, #9-external) simultaneously by removing the divergent path from consensus. The per-finding M/L fixes only matter if the external VM must remain on-path.

## 3. Notable FIXED (consensus-critical record)

- **#0** — Fabricated HF_Faun activation heights (mainnet 8800000 / testnet 12960000) removed; Faun/Gorgon unscheduled on both nets, matching C# config ending at HF_Echidna. Regression tests assert disabled.
- **#1** — Fabricated 12th native contract `TokenManagement` (id -12, HF_Faun) deleted entirely (commit `3ad4e0df`); exactly 11 natives registered, matching C#.
- **#5** — VM `MaxItemSize` default corrected to `(u16::MAX)*2 = 131070` (was 65535); parity + boundary tests present.
- **#10** — neo-core post-instruction MaxStackSize check off-by-one fixed (`>=`→`>`); at exactly 2048 refs neither Rust nor C# faults.
- **#11** — `MaxComparableSize` default corrected to literal 65536 (was 65535); boundary 65536-allowed/65537-faults.
- **#12** — ENDTRY-in-FINALLY now FAULTs in the neo-core JumpTable consensus engine, matching C# (the neo-vm-rs interpreter copy is separate, covered by #13).
- **#14** — Block `NextConsensus` now computed from the refresh-conditional *next-block* validator set (Compute vs Get on committee boundary), not the current signing set; signing multisig kept separate.
- **#15** — `load_validators` (signers) made unconditional; refresh-conditional choice isolated to `next_consensus_validators` (NextConsensus only).
- **#16** — `NamedCurveHash` Keccak256 variants corrected to 122/123 (were 0x18/0x19); old bytes now rejected. Tests pass.
- **#17** — secp256k1 high-s signatures now accepted (low-s normalize before verify) in both verify paths, matching C# malleability behavior. Regression test passes.
- **#18** — Block import no longer re-verifies every transaction; `Block::verify` == header-only, matching C#. Tx-integrity (dup hashes, merkle root) moved to deserialization.
- **#20** — Header verify no longer rejects future timestamps (15-min drift) or small witness scripts; only the C# checks remain. Stale helpers are dead code.
- **#21** — `OracleResponse::verify` now implements all 5 C# checks (request existence, fee match, designated-oracle signer added).
- **#22** — `MaxTransactionsPerBlock` (512) hard import gate removed (commit `59bd7805`); only ushort.MaxValue (65535) wire cap remains.
- **#23** — `StorageItem` `is_constant` flag/constructor/flag-prefixed serialization removed; serializes as raw value bytes, matching C# v3.9.

## 4. STALE

None. Every finding's reported symptom was checked against current code; none were found to be wrong/misattributed. The closest to "stale" are the path-drift notes (auditor cited `neo_csharp/` for VM source that actually lives under `neo_csharp_vm/src/Neo.VM/`, and `neo-core/src/hardfork.rs` is now a re-export shim into `neo-config/`), but in each case the finding's substance was verified at the relocated path — these are location corrections, not invalidations.

### Residual cleanup (no consensus risk) — not counted as OPEN
- **#19 (PARTIAL)** — Both consensus enforcement points (block deserialize + verify) FIXED. Residual: dead `validate_block_size` 2MB gate in `validation.rs:93-113` (no production callers) and misleading `MAX_BLOCK_SIZE`/"ProtocolSettings.Default" comments in `neo-primitives/src/constants.rs:23` & `neo-core/src/constants.rs:47`. Effort **S**, cleanup-only; note `MAX_ARRAY_SIZE`/`MAX_ITEM_SIZE` alias `MAX_BLOCK_SIZE`, so decouple before deleting.