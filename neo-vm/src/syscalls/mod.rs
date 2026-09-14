//! Syscall registry and utilities for NeoVM
//!
//! This module provides static syscall dispatch tables with Cuckoo Hashing for O(1)
//! worst-case lookup time, eliminating per-transaction HashMap allocations.

pub mod static_registry;
pub mod cuckoo_hash;

pub use static_registry::{
    get_gas_cost, get_syscall_entry, init, verify_registry_completeness,
    find_by_method_name, get_method_name, NUM_BUILTINS, GasCost, SyscallEntry, SyscallHash,
};