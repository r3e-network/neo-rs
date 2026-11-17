# Deferred Fixes — neo-ledger

Status of the block-integrity audit items that touch `neo-ledger` and the
`neo-system` blockchain actor. Item IDs are the audit's.

| Item | Status | Where |
| ---- | ------ | ----- |
| B-7  | Done   | commit `3044a5785` |
| Wire integrity vs proposal policy | Done | commit `a33bb1173` |
| B-6  | Closed: no per-transaction re-verification; one hardening applied | commit `341ee8235` |
| V-3  | Done   | commit `91dc74431` |
| B-4  | Done   | the commits whose subject ends in `(B-4)` |

## Done

### B-4: block caches share one retained-bytes budget

Three caches keep whole blocks in memory: the block body cache of
`LedgerContext` (`neo-ledger/src/ledger_context.rs`) and the verified and
unverified block caches of the blockchain actor (`neo-system/src/blockchain`).
Only the unverified cache had a byte bound (256 MiB); the other two were
bounded by entry count, which stopped bounding memory once acceptance was
limited by the 32 MiB payload size instead of 2 MiB.

All three now charge one `BlockCacheBudget`
(`neo-ledger/src/block_cache_budget.rs`), owned by the `LedgerContext` and
picked up by `Blockchain::new`:

* Metric: serialized block size plus 64 bytes, computed once when the block is
  admitted. It approximates heap use and undercounts blocks made of many tiny
  transactions.
* Default: 512 MiB (`DEFAULT_BLOCK_CACHE_BUDGET_BYTES`), twice the existing
  unverified cap, which stays in force inside the budget. The entry-count
  limits are unchanged.
* Accounting: a cached block owns a `BlockCacheReservation` that returns its
  bytes when dropped. A block held by both actor caches shares one
  reservation. Reserving uses checked addition and fails closed; a block
  larger than the whole budget is never cached and evicts nothing.
* Priority: the lower the height, the sooner the chain needs the block. To
  admit a block the actor gives up, in order, cached blocks at or below the
  persisted height, persisted bodies in the ledger context (oldest first), and
  cached blocks above the incoming height (highest first). A block never makes
  one at the same or a lower height give way.
* Admission is all or nothing. Before anything is given up the actor checks
  that the free room and everything the block may displace add up to its
  cost, and that the unverified caps can take it. If not, the block is refused
  and every cache is left as it was, so a block that cannot fit costs the node
  nothing it had cached. Blocks at or below the persisted height are counted
  as room and are given up at the next admission that needs their bytes.
* The next block to persist does not need admission: it is persisted whether or
  not it fits, so blocks far above the tip cannot stall the chain by filling
  the budget.

Caching decides only whether a block has to be downloaded again. A block taken
from an actor cache is still checked by `verify_block_for_persistence` before
it is persisted, and `NeoSystemContext::try_get_block` reads a body missing
from the ledger context from the store.
A verified block that cannot be admitted is answered with `OutOfMemory`, which
the network layer does not treat as a peer fault.

A block that was refused or given up is fetched again. The P2P side learns
neither outcome: the remote node marks the hash as known and reports the
height as delivered before the actor decides. The task manager plans requests
from the persisted height, not from what was delivered, so the height is asked
for again when a delivery or a persisted block makes it plan, and, if nothing
is persisted for ten seconds, by its frontier recovery on a five second timer
(about fifteen seconds with one peer that answers nothing else; up to the 60
second task timeout if three peers already hold a request for that height).
Over the same connection a block sent before is taken again only once it is
the next block to persist, which needs no admission, so such blocks come back
one at a time.

Tests: `neo-ledger/src/block_cache_budget.rs`, the budget tests in
`neo-ledger/src/ledger_context.rs`,
`neo-system/src/blockchain/block_cache_budget_tests.rs`, and, for the fetch
after a block was given up, `neo-system/tests/evicted_block_refetch_tests.rs`
(a whole node against one peer over a loopback connection). The 60 second
path with three peers is not tested.

Not covered: the inventory cache of the actor, the header cache, the actor
mailbox, and blocks the P2P task manager holds per session have their own
limits and are not charged to this budget.

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
