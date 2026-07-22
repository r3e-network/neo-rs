//! # neo-primitives::errors
//!
//! Typed errors and result aliases for this crate boundary.
//!
//! ## Boundary
//!
//! This module belongs to `neo-primitives`. This foundation crate must stay
//! free of node-service, storage-backend, RPC, and network orchestration
//! dependencies.
//!
//! ## Contents
//!
//! - `error`: Typed error definitions and conversions.

pub mod error;
