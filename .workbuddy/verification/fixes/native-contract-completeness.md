# Native Contract Completeness — Verification Report

**Date:** 2026-07-03  
**Review:** neo-rs v3.10.0 engineering review (REVIEW-2026-07-02.md, Section 3)  
**Status:** All three findings already fixed in current code

---

## 1. getBlock missing IsTraceableBlock gate

**Review claim:** `get_block()` should gate on `is_traceable_block()` — the helper exists and is used for transaction getters, but not for getBlock. C# returns null outside the traceable window.

**Verification:** Already present.

**Location:** `neo-native-contracts/src/ledger_contract/mod.rs:369`

```rust
match self.get_trimmed_block(&snapshot, &hash)? {
    Some(block) if self.is_traceable_block(engine, block.index())? => {
        Self::trimmed_block_to_bytes(&block, "getBlock")
    }
    _ => Ok(Vec::new()),
}
```

The `is_traceable_block()` check is applied in the match guard at line 369. When the block is not traceable, the guard fails and the `_ => Ok(Vec::new())` branch returns null (empty payload), matching C# `LedgerContract.GetBlock` behavior:
- Resolves index/hash to block hash
- Gets TrimmedBlock from storage
- Checks `IsTraceableBlock` — returns null if outside window
- Otherwise returns the block

**Git blame:** `77d435d8` (June 14, 2026)

---

## 2. Conflicts-attribute stubs not persisted in on_persist

**Review claim:** `contains_conflict_hash` reads conflict stubs from storage, but new conflicts are never written during `on_persist` — conflict-avoidance breaks over time as old stubs expire.

**Verification:** Already implemented.

**Location:** `neo-blockchain/src/ledger/ledger_records.rs:188-210`

The `write_on_persist_records` function writes, for every transaction in a block:
- A bare-hash conflict stub (keyed by `Prefix_Transaction + conflict_hash`) — line 202
- A per-signer conflict stub (keyed by `Prefix_Transaction + conflict_hash + signer`) — lines 203-208

```rust
let stub = LedgerContract::new().serialize_conflict_stub(index)?;
for conflict_hash in &conflict_hashes {
    upsert(cache, Self::transaction_key(conflict_hash), stub.clone());
    for signer in &signers {
        upsert(cache, Self::conflict_signer_key(conflict_hash, signer), stub.clone());
    }
}
```

This is wired into the OnPersist trigger at `neo-blockchain/src/pipeline/native_persist.rs:418`:

```rust
TriggerType::OnPersist => {
    crate::ledger_records::LedgerRecords::write_on_persist_records(
        &snapshot, block, block_hash,
    )?;
}
```

The architecture note: `LedgerContract::on_persist` is the default no-op because the LedgerContract in `neo-native-contracts` is intentionally read-only. The block-recording writes happen through `LedgerRecords` in `neo-blockchain`, using the same `serialize_conflict_stub` codec that `contains_conflict_hash` reads — ensuring byte-identical storage.

**Git blame:** `ab46ad34` (June 14, 2026) + `78880da1` (June 18, 2026) for pipeline wiring

---

## 3. unclaimedGas boundary check ordering

**Review claim:** Need to confirm that `end == expect_end` faults happen BEFORE the storage lookup (not after). The order matters for correct error handling matching C#.

**Verification:** Already correct.

**Location:** `neo-native-contracts/src/neo_token/invoke.rs:404-422`

```rust
let snapshot = engine.snapshot_cache();                      // line 404
let expect_end = match engine.persisting_block() {           // line 405
    Some(block) => block.index(),                            // line 406
    None => LedgerContract::new()                            // line 407
        .current_index(&snapshot)?                           // line 408
        .saturating_add(1),                                  // line 409
};                                                           // line 410
if end != expect_end {                                       // line 411 ← FAULT HERE
    return Err(CoreError::invalid_operation(format!(         // line 412
        "NeoToken::unclaimedGas: end {end} must equal {expect_end}"  // line 413
    )));                                                     // line 414
}                                                            // line 415
let bonus = match self.read_account_state(&snapshot, &account) {  // line 416 ← STORAGE LOOKUP AFTER
    Some(bytes) => {                                         // line 417
        let state = Self::decode_neo_account_state(&bytes)?; // line 418
        self.calculate_bonus(&snapshot, &state, end)?        // line 419
    }                                                        // line 420
    None => BigInt::from(0),                                 // line 421
};                                                           // line 422
```

The boundary validation (`end != expect_end`) at lines 411-415 **faults before** the storage lookup (`read_account_state`) at line 416. This matches C# `NeoToken.UnclaimedGas`:

```csharp
uint expect_end = engine.PersistingBlock is null ?
    Ledger.CurrentIndex(snapshot) + 1 : engine.PersistingBlock.Index;
if (end != expect_end) throw new ArgumentException(nameof(end));
var state = account.GetAndChange(snapshot).GetInteroperable<NeoAccountState>();
```

Note: The `expect_end` computation itself may do a storage lookup via `current_index()` when there's no persisting block, but this is identical to C#'s `Ledger.CurrentIndex(snapshot)` call at the same position.

**Git blame:** `9b9ee304` (June 20, 2026)

---

## Summary

All three MEDIUM findings from the v3.10.0 engineering review were already fixed before the review was written:

| Finding | File | Status | Commit Date |
|---------|------|--------|-------------|
| getBlock IsTraceableBlock gate | `ledger_contract/mod.rs:369` | Already present | June 14, 2026 |
| Conflicts stubs in on_persist | `ledger_records.rs:188-210` | Already present | June 14-18, 2026 |
| unclaimedGas boundary ordering | `neo_token/invoke.rs:411` | Already correct | June 20, 2026 |

**Compilation:** `cargo check -p neo-native-contracts` — passes cleanly.

**Recommendation:** The review document REVIEW-2026-07-02.md should be updated to mark these three items as verified/resolved, since all were committed before the review date.
