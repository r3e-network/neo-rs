//! # neo-primitives::blockchain
//!
//! Blockchain-domain primitive records used across crates.
//!
//! ## Boundary
//!
//! This module belongs to `neo-primitives`. This foundation crate must stay
//! free of node-service, storage-backend, RPC, and network orchestration
//! dependencies.
//!
//! ## Contents
//!
//! - `marker_traits`: marker traits for primitive domains.

/// Minimal marker traits used to decouple higher-level crates.
pub mod marker_traits;

pub use marker_traits::BlockLike;
