//! # neo-crypto::hashes
//!
//! Hash functions and hash-domain helpers used by protocol code.
//!
//! ## Boundary
//!
//! This module belongs to `neo-crypto`. This foundation crate owns
//! cryptographic primitives and must not depend on node services, RPC, storage
//! engines, or UI crates.
//!
//! ## Contents
//!
//! - `hash`: Neo hash functions and adapters.
//! - `merkle_tree`: Neo block/MerkleBlock tree construction and proof helpers.
//! - `murmur`: Thin Neo error-free adapters over the upstream `murmur3` crate.
//! - `named_curve_hash`: named-curve hash mapping helpers.

pub mod hash;
pub mod merkle_tree;
pub mod murmur;
pub mod named_curve_hash;
