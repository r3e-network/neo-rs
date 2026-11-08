//! Message types re-exported from neo-p2p for backward compatibility.
//!
//! This module provides the message types used by the P2P networking layer.
//! The actual implementations live in the `neo-p2p` crate.

pub use crate::network::p2p::{NetworkMessage, ProtocolMessage};
