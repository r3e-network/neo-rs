//! Property-based tests (proptest) over real neo-rs VM code.
//!
//! Covers Phase C:
//!   1. `PooledHashMap` agrees with `std::collections::HashMap` under a random
//!      sequence of inserts/overwrites/lookups (reference equivalence).
//!   2. Registered syscall gas prices are non-negative (an economic invariant of
//!      the pricing registry `static_registry.rs`).

use neo_vm::memory::pooled_containers::PooledHashMap;
use neo_vm::syscalls::static_registry::{get_gas_cost, init};
use proptest::prelude::*;
use sha2::{Digest, Sha256};
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

    /// Every registered syscall maps to a non-negative gas price.
    /// (Gas prices are an economic invariant; a negative price would be a bug.)
    #[test]
    fn registered_syscall_gas_prices_are_non_negative(
name in prop::sample::select(&[
            "System.Storage.GetContext",
            "System.Storage.AsReadOnly",
            "System.Storage.Get",
            "System.Storage.Put",
            "System.Storage.Delete",
            "System.Runtime.Platform",
            "System.Runtime.GetTime",
            "System.Runtime.CheckWitness",
            "System.Runtime.BurnGas",
            "System.Runtime.Log",
            "System.Contract.Call",
            "System.Contract.Create",
            "System.Crypto.CheckSig",
        ]),
    ) {
        // The syscall registry is lazily initialised (OnceLock); without this
        // get_gas_cost would see an empty table and return None for everything.
        init();
        let hash = Sha256::digest(name.as_bytes());
        let cost = get_gas_cost(&hash.into())
            .unwrap_or_else(|| panic!("known syscall {name} must be registered"));
        prop_assert!(cost >= 0, "gas price must be non-negative for {name}, got {cost}");
    }
}

/// Differential tie to the Coq refinement model (formal/coqlib/gas_price_refinement.v):
/// the Rust registry must return exactly the same concrete prices the Coq model asserts.
#[test]
fn syscall_gas_prices_match_coq_refinement_model() {
    init();
    let cases: &[(&str, i64)] = &[
        ("System.Storage.GetContext", 16),              // 1 << 4
        ("System.Storage.GetReadOnlyContext", 16),      // 1 << 4
        ("System.Storage.AsReadOnly", 16),              // 1 << 4
        ("System.Storage.Get", 32768),                  // 1 << 15
        ("System.Storage.Put", 32768),                  // 1 << 15
        ("System.Storage.Find", 32768),                 // 1 << 15
        ("System.Runtime.Platform", 8),                 // 1 << 3
        ("System.Runtime.GetTime", 8),                  // 1 << 3
        ("System.Runtime.CurrentSigners", 8),           // 1 << 3
        ("System.Runtime.CheckWitness", 1024),          // 1 << 10
        ("System.Runtime.BurnGas", 16),                 // 1 << 4
        ("System.Runtime.GetRandom", 16),               // 1 << 4
        ("System.Runtime.Log", 32768),                  // 1 << 15
        ("System.Contract.Call", 32768),                // 1 << 15
        ("System.Contract.GetCallFlags", 16),           // 1 << 4
        ("System.Contract.CreateStandardAccount", 256), // 1 << 8
        ("System.Contract.CreateMultisigAccount", 512), // 1 << 9
        ("System.Contract.NativePostPersist", 1024),    // 1 << 10
        ("System.Crypto.CheckSig", 32768),              // 1 << 15
        ("System.Iterator.Next", 32768),                // 1 << 15
    ];
    for (name, expected) in cases {
        let hash = Sha256::digest(name.as_bytes());
        let actual = get_gas_cost(&hash.into())
            .unwrap_or_else(|| panic!("known syscall {name} must be registered"));
        assert_eq!(
            actual, *expected,
            "gas price for {name} must match the Coq refinement model"
        );
    }
}
