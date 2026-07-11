//! # Static Ledger archive
//!
//! ## Boundary
//!
//! The protocol-blind static-file crate stores opaque bytes. This module is the
//! only adapter that knows which C#-compatible Ledger rows make a block and
//! its transaction/conflict queries complete. Construction goes through
//! [`StaticLedgerArchiveFactory`]; reads go through the ordinary Ledger
//! provider traits.
//!
//! ## Contents
//!
//! - `archive`: Typed Ledger facade over static records.
//! - `capture`: Exact finalized Ledger-row extraction.
//! - `factory`: Configured archive construction.
//! - `recovery`: Canonical hot-prefix reconciliation.

mod archive;
mod capture;
mod factory;
mod recovery;

pub use archive::StaticLedgerArchive;
pub use factory::StaticLedgerArchiveFactory;
pub use recovery::StaticArchiveRecovery;
