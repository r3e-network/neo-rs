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
//! - `pruning`: Version-aware atomic removal of archived hot Ledger rows.
//! - `recovery`: Canonical hot-prefix reconciliation.

mod archive;
mod capture;
mod factory;
mod pruning;
mod recovery;

pub use archive::StaticLedgerArchive;
pub use factory::StaticLedgerArchiveFactory;
pub use pruning::HotLedgerPruneOutcome;
pub use recovery::StaticArchiveRecovery;
