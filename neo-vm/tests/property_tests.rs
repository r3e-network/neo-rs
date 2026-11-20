//! Property-based tests (proptest) over real neo-rs VM code.
//!
//! Covers Phase C: a `PooledHashMap` must agree with `std::collections::HashMap`
//! under a random sequence of inserts/overwrites/lookups (reference
//! equivalence).
//!
//! Syscall gas prices are no longer modelled here: the VM stopped owning a
//! syscall price table, so the economic-invariant and Coq-refinement checks
//! live with the registration authority in `neo-smart-contract`.

use neo_vm::memory::pooled_containers::PooledHashMap;
use proptest::prelude::*;
use std::collections::HashMap;

proptest! {
    /// A `PooledHashMap` must behave identically to a standard `HashMap` for
    /// inserts, overwrites, lookups and length, across a random op sequence.
    /// Capacity is fixed and pre-sized so inserts never panic.
    #[test]
    fn pooled_hashmap_matches_std(
        ops in prop::collection::vec((any::<u32>(), any::<i64>()), 1..100),
    ) {
        let mut pooled = PooledHashMap::<u32, i64>::new(256);
        let mut std_map = HashMap::<u32, i64>::new();

        for (k, v) in &ops {
            let old_pooled = pooled.insert_no_alloc(*k, *v);
            let old_std = std_map.insert(*k, *v);
            prop_assert_eq!(old_pooled, old_std, "overwrite return for key {:?}", k);
            prop_assert_eq!(pooled.get_no_alloc(k), std_map.get(k), "lookup {:?}", k);
            prop_assert_eq!(pooled.len(), std_map.len(), "len after insert {:?}", k);
        }

        // Full-key-space scan agrees with std.
        for k in 0u32..1024u32 {
            prop_assert_eq!(pooled.get_no_alloc(&k), std_map.get(&k), "scan {:?}", k);
        }
    }
}
