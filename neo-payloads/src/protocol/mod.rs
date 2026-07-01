//! # neo-payloads::protocol
//!
//! Protocol enums, versioned records, and chain-level domain constants.
//!
//! ## Boundary
//!
//! This module belongs to `neo-payloads`. This protocol crate owns payload
//! records and validation helpers and must not perform IO, storage commits, or
//! service orchestration.
//!
//! ## Contents
//!
//! - `extensible_payload`: extensible payload records.
//! - `inventory`: inventory payload traits and records.

/// Extensible payload for consensus.
pub mod extensible_payload;
/// Inventory interface trait.
pub mod inventory;
