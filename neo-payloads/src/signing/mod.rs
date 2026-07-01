//! # neo-payloads::signing
//!
//! Witness, signer, and signature validation helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-payloads`. This protocol crate owns payload
//! records and validation helpers and must not perform IO, storage commits, or
//! service orchestration.
//!
//! ## Contents
//!
//! - `helper`: shared helper functions.
//! - `signer`: signer configuration and signing helpers.
//! - `verifiable_ext`: verifiable payload extension helpers.
//! - `witness`: witness records and serialization helpers.
//! - `witness_rule`: witness rule records and evaluation helpers.

/// Helper utilities for signing / computing the sign-data buffer.
pub mod helper;
/// Transaction signer structure.
pub mod signer;
/// Extension of [`neo_primitives::Verifiable`] with payload-level helpers.
pub mod verifiable_ext;
/// Witness attached to verifiable payloads.
pub mod witness;
/// Witness rules and conditions used by transaction signers.
pub mod witness_rule;
