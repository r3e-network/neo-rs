//! Contract — re-exported from `neo-vm`.
//!
//! The `Contract` type was moved to `neo-vm` so that `neo-wallets` can use it
//! without depending on `neo-execution`. This module re-exports it for backward
//! compatibility with existing `use neo_execution::Contract` imports.

pub use neo_vm::Contract;
