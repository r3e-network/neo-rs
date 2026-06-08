//! # neo-block
//!
//! Canonical home for the pure, dependency-light block-layer data types
//! that are not coupled to the stateful consensus engine in `neo-core`.
//!
//! Currently contains:
//! - [`VerifyResult`] — pure outcome enum (re-exported from `neo-primitives`).
//! - [`TransactionState`] — per-transaction state produced by the
//!   executor (matches `Neo.Ledger.TransactionState`).
//! - [`ApplicationExecuted`] — per-transaction execution result with
//!   notifications and logs.
//! - [`NotifyEventArgs`] — rich notification event payload (re-exported
//!   from `neo_execution` for plugin consumption).

#![doc(html_root_url = "https://docs.rs/neo-block/0.7.2")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod application_executed;
pub mod notify_event_args;
pub mod transaction_state;
pub mod verify_result;

pub use application_executed::ApplicationExecuted;
pub use notify_event_args::NotifyEventArgs;
pub use transaction_state::TransactionState;
pub use verify_result::VerifyResult;
