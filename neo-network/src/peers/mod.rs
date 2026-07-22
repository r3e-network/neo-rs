//! # neo-network::peers
//!
//! Peer registry, scoring, and connection tracking logic.
//!
//! ## Boundary
//!
//! This module belongs to `neo-network`. This service crate owns P2P transport
//! and peer behavior and must not execute blocks, own consensus rules, or
//! mutate storage directly.
//!
//! ## Contents
//!
//! - `connection_timeouts`: Peer connection timeout policy helpers.
//! - `peer_id`: Peer identifier records and conversions.
//! - `peer_registry`: Peer registry storage and lookup helpers.

pub mod connection_timeouts;
pub mod peer_id;
pub mod peer_registry;

pub use connection_timeouts::ConnectionTimeouts;
pub use peer_id::PeerId;
pub use peer_registry::{ConnectedPeerSnapshot, PeerRegistry};
