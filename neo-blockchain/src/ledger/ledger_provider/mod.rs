//! # neo-blockchain::ledger::ledger_provider
//!
//! Provider-style read API over persisted Neo ledger records.
//!
//! ## Boundary
//!
//! This module belongs to `neo-blockchain`. It owns read-only ledger
//! capabilities over hot native Ledger records and cold provider-compatible
//! archives, but it does not persist new blocks or choose pruning policy.
//!
//! ## Contents
//!
//! - `empty`: Clean-miss provider/factory for nodes without a cold archive.
//! - `traits`: Read capability traits and typed provider factory contract.
//! - `storage`: Hot provider over native Ledger records in a `DataCache`.
//! - `static_file`: Cold provider over append-only finalized Ledger records.
//! - `hot_cold`: Read router that falls back to a cold provider only when hot
//!   native Ledger records miss.
//! - `optional`: Static optional-provider composition for runtime config.

mod empty;
mod hot_cold;
mod optional;
mod static_file;
mod storage;
mod traits;

pub use empty::{EmptyLedgerProvider, EmptyLedgerProviderFactory};
pub use hot_cold::{HotColdLedgerProvider, HotColdLedgerProviderFactory};
pub use optional::OptionalLedgerProvider;
pub use static_file::{StaticLedgerProvider, StaticLedgerProviderFactory};
pub use storage::{StorageLedgerProvider, StorageLedgerProviderFactory};
pub use traits::{
    BlockProvider, ChainTipProvider, LedgerProvider, LedgerProviderFactory,
    TransactionStateProvider, TxProvider,
};

/// Runtime-selectable static Ledger fallback without trait-object dispatch.
///
/// `Disabled` is the explicit no-archive configuration; `Enabled` carries the
/// concrete append-only provider opened by the application composition root.
pub type OptionalStaticLedgerProvider = OptionalLedgerProvider<StaticLedgerProvider>;

#[cfg(test)]
#[path = "../../tests/ledger/ledger_provider.rs"]
mod tests;
