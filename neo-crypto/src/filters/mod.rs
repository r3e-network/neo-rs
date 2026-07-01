//! # neo-crypto::filters
//!
//! Probabilistic filters and related helpers used by networking and indexes.
//!
//! ## Boundary
//!
//! This module belongs to `neo-crypto`. This foundation crate owns
//! cryptographic primitives and must not depend on node services, RPC, storage
//! engines, or UI crates.
//!
//! ## Contents
//!
//! - `bloom_filter`: Bloom filter implementation and codecs.

/// Bloom filter implementation for probabilistic set membership testing.
pub mod bloom_filter;
