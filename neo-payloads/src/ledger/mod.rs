//! # neo-payloads::ledger
//!
//! Ledger caches, lookup context, and persisted record helpers used by block
//! import.
//!
//! ## Boundary
//!
//! This module belongs to `neo-payloads`. This protocol crate owns payload
//! records and validation helpers and must not perform IO, storage commits, or
//! service orchestration.
//!
//! ## Contents
//!
//! - `block`: block payload records and serialization helpers.
//! - `header`: header payload records and serialization helpers.
//! - `headers_payload`: headers payload records.
//! - `merkle_block_payload`: Merkle block payload records.
//! - `transaction_state`: transaction state records.
//! - `trimmed_block`: trimmed block records.

/// Block structure and structural verification.
pub mod block;
/// Block header structure and structural verification.
pub mod header;
/// Headers response payload.
pub mod headers_payload;
/// Merkle block payload for SPV.
pub mod merkle_block_payload;
/// Ledger transaction state record used by `LedgerContract` storage.
pub mod transaction_state;
/// Trimmed block (header + transaction hashes) used by LedgerContract storage.
pub mod trimmed_block;
