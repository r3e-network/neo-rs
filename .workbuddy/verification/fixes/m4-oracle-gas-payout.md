# M4: Oracle PostPersist GAS Payout Verification

**Status: NO FIX NEEDED — already correct**

**Date:** 2026-07-03  
**Severity:** MEDIUM divergence (alleged)  
**Verified against:** C# NEO v3.10.0 `OracleContract.cs:PostPersistAsync`

---

## Summary

The Rust implementation of `OracleContract::post_persist()` in `neo-native-contracts/src/oracle_contract/mod.rs` is already complete and correct. The PostPersist hook properly handles GAS distribution from the oracle request fee to the responding Oracle nodes. No gap exists.

---

## Implementation Analysis

### PostPersist Hook (`neo-native-contracts/src/oracle_contract/mod.rs:147-233`)

The `post_persist` method implements the full C# `OracleContract.PostPersistAsync` logic:

```
fn post_persist(&self, engine: &mut ApplicationEngine) -> CoreResult<()>
```

**Step-by-step comparison:**

| Step | C# v3.10.0 (`OracleContract.cs:PostPersistAsync`) | Rust (`mod.rs:147-233`) | Match? |
|------|---------------------------------------------------|------------------------|--------|
| 1. Iterate block transactions | `foreach (Transaction tx in snapshot.PersistingBlock.Transactions)` | `block.transactions.iter().filter_map(...)` | Yes |
| 2. Filter OracleResponse txs | `tx.GetAttribute<OracleResponse>()` | `Self::oracle_response_attribute(tx)` | Yes |
| 3. Skip missing responses | `if (response is null) continue` | `let Some(item) = snapshot.get(&key) else { continue }` | Yes |
| 4. Remove request from storage | `snapshot.Delete(CreateStorageKey(...).Add(response.Id))` | `snapshot.delete(&key)` | Yes |
| 5. Remove id from per-url id-list | `IdList.Remove(response.Id)` + delete if empty | `list.remove(position)` + delete if empty | Yes |
| 6. Fetch oracle nodes | `RoleManagement.GetDesignatedByRole(snapshot, Role.Oracle, snapshot.Height)` | `RoleManagement::new().get_designated_by_role_at(&snapshot, Role::Oracle, block_index)` | Yes |
| 7. Select node by `id % nodes.len()` | `nodes[response.Id % nodes.Length]` | `id % nodes.len()` | Yes |
| 8. Accumulate oracle price to node | `oracleNodes[account] += GetPrice(snapshot)` | `nodes[index].1 += BigInt::from(price)` | Yes |
| 9. Mint GAS to accumulated accounts | `GAS.Mint(engine, account, value, false)` | `GasToken::new().gas_mint(engine, &account, &gas, false)` | Yes |
| 10. Return void | `async void` (no return) | `Ok(())` | Yes |

### Designation Lookup (`role_management/storage.rs:17-36`)

`find_designation_value` scans backward in descending index order to find the designation with the greatest index <= the given height. Matches C# `FindRange((role, index), (role), Backward).FirstOrDefault()`.

### Node Account Derivation

| C# | Rust | Match? |
|----|------|--------|
| `Contract.CreateSignatureRedeemScript(nodes[response.Id % nodes.Length])` | `Contract::create_signature_redeem_script(point)` | Yes |

---

## Test Coverage

Two tests directly verify PostPersist behavior:

### `post_persist_removes_answered_requests_and_id_list_entries` (`request_finish_tests.rs:517-576`)

- Verifies request removal from storage after response
- Verifies id-list entry removal (partial and full)
- Verifies empty id-list deletion
- Verifies no-fault on response without stored request

### `post_persist_mints_the_price_to_the_designated_oracle_node` (`request_finish_tests.rs:578-607`)

- Designates an Oracle node at index 0
- Creates a block with an OracleResponse transaction for request id 7
- Runs PostPersist
- Asserts the oracle node receives `DEFAULT_ORACLE_PRICE` (0.5 GAS = 50000000 datoshi)

**All 24 oracle contract tests pass (0 failures).**

---

## Why This Was Flagged (Context)

The parity findings document (`claudedocs/spec-v3100-parity-findings.md`) identifies multiple Oracle-related divergences in the Python spec (`native/oracle.py`), including:

1. `Oracle.request` omits fee charging, GAS mint, and contract-caller check
2. `Oracle.finish` missing invocation-stack/counter guards and OracleResponse notification
3. OracleRequest stored with wrong binary layout

However, **none of these findings mention PostPersist as missing**. The Rust implementation in `neo-native-contracts` had already addressed all Oracle spec divergences during or before the v0.9.0 review cycle:

- `request()` at `mod.rs:261-432`: includes fee charging (`charge_execution_fee`), GAS mint (`gas_mint` to oracle contract hash), contract-caller check (`is_contract`), proper user_data serialization, and OracleRequest notification
- `finish()` at `mod.rs:434-506`: includes invocation-stack/counter guards, OracleResponse notification, `CallFromNativeContract`-equivalent callback, and does NOT remove the request (PostPersist does)
- `post_persist()` at `mod.rs:147-233`: full implementation as documented above

The M4 divergence claim likely originated from concern that the PostPersist GAS payout was not implemented, but the Rust code already had this in place.

---

## Conclusion

**No fix required.** The `OracleContract::post_persist()` implementation in `neo-native-contracts/src/oracle_contract/mod.rs:147-233` correctly implements C# v3.10.0 `OracleContract.PostPersistAsync`, including:

- Identification of OracleResponse transactions in the persisting block
- Removal of answered request records and id-list entries
- Lookup of designated Oracle nodes via RoleManagement at the correct block height
- Round-robin node selection by `id % nodes.len()`
- Bulk accumulation and minting of oracle price GAS to the designated node accounts

All 24 oracle contract tests pass, including two dedicated PostPersist tests that verify both storage cleanup and GAS payout behavior.
