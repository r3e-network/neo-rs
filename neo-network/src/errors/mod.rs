//! # neo-network::errors
//!
//! Typed errors and result aliases for this crate boundary.
//!
//! ## Boundary
//!
//! This module belongs to `neo-network`. This service crate owns P2P transport
//! and peer behavior and must not execute blocks, own consensus rules, or
//! mutate storage directly.
//!
//! ## Contents
//!
//! - `error`: Typed error definitions and conversions.

pub mod error;

pub use error::{NetworkError, NetworkResult};
