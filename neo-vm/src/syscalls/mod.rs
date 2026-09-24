//! Syscall registry and utilities for NeoVM
//!
//! This module provides static syscall dispatch tables with Cuckoo Hashing for O(1)
//! worst-case lookup time, eliminating per-transaction HashMap allocations.

pub mod cuckoo_hash;
pub mod static_registry;

pub use static_registry::{
    GasCost, NUM_BUILTINS, SyscallEntry, SyscallHash, find_by_method_name, get_gas_cost,
    get_method_name, get_syscall_entry, get_syscall_entry_by_u32, init,
    verify_registry_completeness,
};
