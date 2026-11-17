# Deferred Fixes — neo-ledger

Status of the block-integrity audit items that touch `neo-ledger` and the
`neo-system` blockchain actor. Item IDs are the audit's.

| Item | Status | Where |
| ---- | ------ | ----- |
| B-7  | Done   | commit `3044a5785` |
| Wire integrity vs proposal policy | Done | commit `a33bb1173` |
| B-6  | Closed: no per-transaction re-verification; one hardening applied | commit `341ee8235` |
| V-3  | Done   | the commit that last touched this file |
| B-4  | **Open** | not addressed here |

## Still open

### B-4

Not addressed by any of the changes described in this file. It is owned by a
separate work stream that starts after this one; this file only records that
it is still open.

## Done

### B-7: trusted import skipped body-integrity checks

`Blockchain::handle_import` (`neo-system/src/blockchain/handlers.rs`) bypasses
`on_new_block`. With `import.verify == false` the only check before execution
was the next-height classification, and the import message carries typed
execution blocks that need not have come through the wire decoder.

"Trusted" now means "skip header verification" (consensus witness, prev-hash
linkage, timestamp, primary index), as in C# `Blockchain.OnImport(blocks,
verify: false)`. Every imported block is checked with
`verify_execution_block_integrity` before anything executes:

* supported block version;
* at most 65,535 transactions (`ushort.MaxValue` in C# `Block.Deserialize`);
* no duplicate transaction hashes;
* transaction merkle root equal to the header's (zero for an empty block);
* exactly one header witness, checked on the execution header before
  projection, because the payload projection keeps the first witness and
  fabricates an empty one for an empty list. Encoding check only; the witness
  is not executed.

A failure is reported as `ImportError::VerificationFailure`; partial-import
reporting is unchanged. Tests: `neo-system/src/blockchain/import_integrity_tests.rs`.

### Wire integrity vs proposal policy

The former `verify_block_structure` also enforced 512 transactions per block
and a 2 MiB block size on the acceptance path. In C# those limits
(`ProtocolSettings.MaxTransactionsPerBlock`, dBFT `MaxBlockSize`) constrain
what a consensus node proposes; block acceptance does not check them.

`neo-ledger/src/validation/block.rs` now has two separate pieces:

* `verify_block_integrity` / `verify_execution_block_integrity`: the wire
  rules listed under B-7. Used by `on_new_block`, `verify_block`,
  `verify_block_with_cache`, the pre-persist check and import.
* `verify_block_proposal_policy`: the 512-transaction and 2 MiB limits. Not on
  any acceptance path. Consensus code in `neo-node` / `neo-consensus` applies
  the same limits from runtime settings when proposing and when validating a
  prepare request.

Effective size bound on acceptance: the 32 MiB P2P payload limit plus the
65,535 transaction cap; the RPC server's request-body limit for `submitblock`.

### B-6: transaction re-verification at block acceptance

Conclusion: not required, and not added. C# does not re-verify each
transaction when accepting a block; the consensus witness covers the header,
and the header's merkle root commits to the transaction hashes.

What that argument needs is that the supplied body is the one the merkle root
commits to. `Blockchain::verify_block_for_persistence`
(`neo-system/src/blockchain/mod.rs`) had a fast path that accepted a block
when its header hash matched an already-verified cached header. A header hash
commits to the merkle root, not to the body supplied with it, so the body
integrity check now runs before that fast path. Normal ingress already
performs the same check, so this is defence in depth, not a fix for a
reachable fault. Regression test:
`pre_persist_verification_rejects_substituted_body_under_verified_cached_header`.

### V-3: `MerkleTree::compute_root(...).expect(...)` in block validation tests

The test helper `merkle_root_of` in `neo-ledger/src/validation/block.rs`
panicked on an empty transaction list because `MerkleTree::compute_root`
returns `None` for an empty slice. It now returns `UInt256::zero()`, the root
C# uses for an empty block, and
`merkle_root_of_empty_transaction_list_is_zero` pins that.

No change was needed in `neo-crypto`: `MerkleTree::compute_root` was already
non-panicking, and production call sites handle `None` explicitly.
