//! # neo-blockchain::ledger
//!
//! Ledger caches, lookup context, and persisted record helpers used by block
//! import.
//!
//! ## Boundary
//!
//! This module belongs to `neo-blockchain`. This node-service crate owns the
//! concrete block-import path and must not depend upward on composition, RPC,
//! GUI, or binaries.
//!
//! ## Contents
//!
//! - `header_cache`: header lookup cache and height/hash indexes.
//! - `ledger_context`: ledger context facade for block import.
//! - `ledger_provider`: provider-style read traits over hot ledger records.
//! - `ledger_records`: persisted ledger record codecs.
//! - `provider_factory`: factories for hot/cold ledger provider views.
//! - `pruning`: consumer acknowledgements for future hot ledger pruning.
//! - `static_archive`: append-only cold block and transaction body archive.

pub mod header_cache;
pub mod ledger_context;
pub mod ledger_provider;
pub(crate) mod ledger_records;
pub mod provider_factory;
pub mod pruning;
pub mod static_archive;
