//! # neo-static-files tests
//!
//! ## Boundary
//!
//! This harness validates the protocol-blind archive provider and its durable
//! index. Higher-level Neo Ledger reconciliation remains in `neo-blockchain`.
//!
//! ## Contents
//!
//! - `archive`: Append, lookup, ownership, recovery, and MDBX index behavior.

mod archive;
