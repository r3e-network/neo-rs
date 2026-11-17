# Deferred Fixes — neo-ledger

Fixes that were identified but intentionally not applied in the current task
because the target file carries uncommitted changes owned by another work
stream. Apply them once that file is committed or the work stream lands.

## V-3 (MEDIUM): `MerkleTree::compute_root(...).expect(...)` in block validation tests

### Location

`neo-ledger/src/validation/block.rs`, `#[cfg(test)] mod tests`,
helper `merkle_root_of` (around lines 99-102):

```rust
fn merkle_root_of(transactions: &[Transaction]) -> UInt256 {
    let hashes: Vec<UInt256> = transactions.iter().map(Transaction::hash).collect();
    MerkleTree::compute_root(&hashes).expect("non-empty transaction list")
}
```

### Analysis

* `MerkleTree` lives in `neo-crypto/src/merkle_tree.rs`.
* `MerkleTree::compute_root(&[UInt256]) -> Option<UInt256>` is already
  non-panicking: it returns `None` for an empty slice and `Some(root)`
  otherwise.
* `MerkleTree::new` / `MerkleTree::compute_root_with_tree` are likewise
  non-panicking; `root()` returns `Option<&UInt256>`.
* Therefore no change is required inside `neo-crypto`. The only panic path is
  the call-site `.expect(...)`, which lives in a test helper.
* Production call sites already handle the empty case explicitly, e.g.
  `neo-protocol/src/payloads/block/structural.rs` returns `false` on `None`,
  `neo-protocol/src/payloads/block.rs::try_rebuild_merkle_root` writes
  `UInt256::default()`, and `neo-ledger/src/validation/rules.rs` matches on
  `Some`/`None`.

### Why deferred

`neo-ledger/src/validation/block.rs` has uncommitted changes in the current
work tree, so editing it would conflict with the owning work stream.

### Recommended patch (when unblocked)

Align with C# `MerkleTree.ComputeRoot`, which yields `UInt256.Zero` for an
empty leaf set:

```rust
fn merkle_root_of(transactions: &[Transaction]) -> UInt256 {
    let hashes: Vec<UInt256> = transactions.iter().map(Transaction::hash).collect();
    MerkleTree::compute_root(&hashes).unwrap_or_else(UInt256::zero)
}
```

`UInt256::zero()` is already used this way in
`neo-rpc-server/src/rpc_server_blockchain/tests.rs` and
`neo-rpc-server/tests/rpc_blockchain_getrawtransaction_vmstate.rs`.

### Verification

`neo-ledger/src/validation/block_validation_refinement_tests.rs` already pins
the `None`-on-empty contract
(`test_merkle_root_empty_transactions`), so the patch needs no new test.
