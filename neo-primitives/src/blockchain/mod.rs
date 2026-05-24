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
//! - `IMessage`, `IBlock`, `IHeader`: Marker traits for associated types
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

use crate::{UInt160, UInt256};

pub use errors::*;
pub use peer::*;
pub use marker_traits::*;
pub use service_traits::*;

pub mod errors;
pub mod peer;
pub mod marker_traits;
pub mod service_traits;

#[cfg(test)]
mod tests;
