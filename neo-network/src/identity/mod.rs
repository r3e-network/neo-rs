//! # neo-network::identity
//!
//! Peer identity, node keys, and advertised endpoint helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-network`. This service crate owns P2P transport
//! and peer behavior and must not execute blocks, own consensus rules, or
//! mutate storage directly.
//!
//! ## Contents
//!
//! - `local_identity`: Local node identity and key material helpers.

pub mod local_identity;

pub use local_identity::LocalIdentity;
