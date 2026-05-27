//! Blockchain and P2P service traits for Neo blockchain.
//!
//! This module provides traits for blockchain access and peer management,
//! breaking the circular dependency between neo-p2p and neo-core
//! (Chain 3: `LocalNode` → Blockchain ↔ `PeerManagerService`).
//!
//! # Design
//!
//! - `BlockchainProvider`: Query and relay operations for blockchain
//! - `PeerRegistry`: Peer management and message broadcasting
//! - `NetworkMessage`, `BlockLike`, `HeaderLike`: Marker traits for associated types
//!
//! # Example
//!
//! ```rust,ignore
//! use neo_primitives::{BlockchainProvider, PeerRegistry};
//! use std::sync::Arc;
//!
//! // LocalNode can be generic over these traits
//! struct LocalNode<B, P>
//! where
//!     B: BlockchainProvider,
//!     P: PeerRegistry,
//! {
//!     blockchain: Arc<B>,
//!     peers: Arc<P>,
//! }
//! ```

pub use errors::*;
pub use marker_traits::*;
pub use peer::*;
pub use service_traits::*;

/// Blockchain and relay error types.
pub mod errors;
/// Peer identity and endpoint metadata.
pub mod peer;
/// Minimal marker traits used to decouple higher-level crates.
pub mod marker_traits;
/// Service traits for blockchain and peer registry access.
pub mod service_traits;

#[cfg(test)]
mod tests;
