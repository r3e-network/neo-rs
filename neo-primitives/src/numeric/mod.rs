//! # neo-primitives::numeric
//!
//! Fixed-size numeric wrappers and byte-order conversion helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-primitives`. This foundation crate must stay
//! free of node-service, storage-backend, RPC, and network orchestration
//! dependencies.
//!
//! ## Contents
//!
//! - `base58_check`: Base58Check encoding helpers.
//! - `big_decimal`: decimal numeric wrapper.
//! - `hex_util`: general-purpose hex encoding/decoding (ADR-024).
//! - `uint160`: UInt160 primitive wrapper.
//! - `uint256`: UInt256 primitive wrapper.
//! - `uint_hex`: hex formatting for fixed-width integers (internal).

pub mod base58_check;
pub mod big_decimal;
pub mod hex_util;
pub mod uint160;
pub mod uint256;
pub(crate) mod uint_hex;
