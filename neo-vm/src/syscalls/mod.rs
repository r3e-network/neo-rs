//! Syscall registry and utilities for NeoVM
//!
//! This module provides static syscall dispatch tables that eliminate per-transaction
//! HashMap allocations. All built-in syscalls are pre-computed at startup using OnceLock.

pub mod static_registry;

pub use static_registry::{
    get_gas_cost, get_syscall_entry, init, verify_registry_completeness,
    find_by_method_name, get_method_name, NUM_BUILTINS, GasCost, SyscallEntry, SyscallHash,
};