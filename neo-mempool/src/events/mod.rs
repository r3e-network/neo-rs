//! # neo-mempool::events
//!
//! Mempool event records emitted to subscribers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-mempool`. This service crate owns transaction
//! pool policy and must not persist blocks, run consensus, or expose RPC
//! transport details.
//!
//! ## Contents
//!
//! - `new_transaction_event_args`: new transaction event args types and
//!   helpers.
//! - `transaction_removed_event_args`: transaction removed event args types and
//!   helpers.

pub mod new_transaction_event_args;
pub mod transaction_removed_event_args;

pub use new_transaction_event_args::NewTransactionEventArgs;
pub use transaction_removed_event_args::TransactionRemovedEventArgs;
