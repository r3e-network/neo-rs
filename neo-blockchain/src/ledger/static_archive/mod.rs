//! Finalized Ledger-record capture and static-file recovery.
//!
//! The generic static-file crate stores opaque bytes. This module is the only
//! adapter that knows which C#-compatible Ledger rows make a block and its
//! transaction/conflict queries complete.

mod archive;
mod capture;
mod recovery;

pub use archive::StaticLedgerArchive;
pub use recovery::StaticArchiveRecovery;
