//! Integration test for syscall dispatch optimization
//!
//! Verifies that the two-tier lookup strategy (fast cuckoo hash for built-ins,
//! fallback HashMap for dynamic syscalls) works correctly.

use neo_vm::InteropService;
use neo_vm::syscalls;
use sha2::{Digest, Sha256};

#[test]
fn test_builtin_syscall_fast_path() {
    // Initialize the syscall registry
    syscalls::init();

    // Test that all built-in syscalls can be looked up via u32 hash
    let builtin_names = vec![
        "System.Runtime.Platform",
        "System.Runtime.GasLeft",
        "System.Storage.Get",
        "System.Storage.Put",
        "System.Contract.Call",
        "System.Crypto.CheckSig",
        "System.Iterator.Next",
    ];

    for name in builtin_names {
        let digest = Sha256::digest(name.as_bytes());
        let hash_u32 = u32::from_le_bytes([digest[0], digest[1], digest[2], digest[3]]);

        // Verify fast path lookup works
        let entry = syscalls::get_syscall_entry_by_u32(hash_u32);
        assert!(entry.is_some(), "Built-in syscall {} should be found", name);

        let entry = entry.unwrap();
        assert!(
            entry.gas_price > 0,
            "Syscall {} should have gas price",
            name
        );

        // Verify method name matches
        let method_name = syscalls::get_method_name(entry.method_id);
        assert_eq!(method_name, name, "Method name should match");
    }
}

#[test]
fn test_unknown_syscall_returns_none() {
    syscalls::init();

    // Test that unknown syscall hash returns None
    let unknown_hash = 0xDEADBEEF_u32;
    let result = syscalls::get_syscall_entry_by_u32(unknown_hash);
    assert!(result.is_none(), "Unknown syscall should return None");
}

#[test]
fn test_interop_service_initialization() {
    // Creating InteropService should initialize the syscall registry
    let _service = InteropService::new();

    // Verify registry is initialized by checking a known syscall
    let digest = Sha256::digest(b"System.Runtime.Platform");
    let hash_u32 = u32::from_le_bytes([digest[0], digest[1], digest[2], digest[3]]);

    let entry = syscalls::get_syscall_entry_by_u32(hash_u32);
    assert!(
        entry.is_some(),
        "Registry should be initialized after InteropService::new()"
    );
}

#[test]
fn test_all_builtin_syscalls_present() {
    syscalls::init();

    // Verify all NUM_BUILTINS syscalls are accessible
    for method_id in 0..syscalls::NUM_BUILTINS {
        let name = syscalls::get_method_name(method_id);
        let entry = syscalls::find_by_method_name(name);
        assert!(
            entry.is_some(),
            "Built-in syscall {} (id={}) should be present",
            name,
            method_id
        );
    }
}

#[test]
fn test_syscall_gas_costs_consistency() {
    syscalls::init();

    // Verify that gas costs are consistent between lookup methods
    let test_syscalls = vec![
        "System.Storage.Get",
        "System.Runtime.Platform",
        "System.Contract.Call",
    ];

    for name in test_syscalls {
        // Lookup by name
        let entry_by_name = syscalls::find_by_method_name(name).unwrap();

        // Lookup by u32 hash
        let digest = Sha256::digest(name.as_bytes());
        let hash_u32 = u32::from_le_bytes([digest[0], digest[1], digest[2], digest[3]]);
        let entry_by_u32 = syscalls::get_syscall_entry_by_u32(hash_u32).unwrap();

        // Verify consistency
        assert_eq!(
            entry_by_name.gas_price, entry_by_u32.gas_price,
            "Gas price mismatch for {}",
            name
        );
        assert_eq!(
            entry_by_name.call_flags, entry_by_u32.call_flags,
            "Call flags mismatch for {}",
            name
        );
        assert_eq!(
            entry_by_name.method_id, entry_by_u32.method_id,
            "Method ID mismatch for {}",
            name
        );
    }
}
