//! # neo-crypto::curves
//!
//! Elliptic-curve adapters and point types used by Neo cryptography.
//!
//! ## Boundary
//!
//! This module belongs to `neo-crypto`. This foundation crate owns
//! cryptographic primitives and must not depend on node services, RPC, storage
//! engines, or UI crates.
//!
//! ## Contents
//!
//! - `bls12381_point`: BLS12-381 point representation and conversions.
//! - `ecc`: elliptic-curve point operations and encoding.

pub mod bls12381_point;
pub mod ecc;
