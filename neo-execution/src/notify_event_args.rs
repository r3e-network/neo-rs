//! Re-export of [`NotifyEventArgs`] from `neo_block`.
//!
//! The canonical home of `NotifyEventArgs` is now `neo_block` (so that
//! ledger-level consumers can use it without taking a dependency on the
//! full `neo_execution` engine crate). This shim keeps the historical
//! `neo_execution::NotifyEventArgs` import path working.

pub use neo_block::NotifyEventArgs;
