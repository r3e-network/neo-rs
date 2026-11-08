//! P2P event handler types.
//!
//! Distributed from `neo-core::events::handlers`.
//!
//! The `MessageReceivedHandler` trait uses a lightweight [`NetworkMessageRef`]
//! instead of the full neo-core `Message` type (which carries compression and
//! serialization concerns). This keeps neo-p2p free of neo-core dependencies
//! while preserving the handler's ability to inspect the message command and
//! raw payload.

use crate::MessageCommand;
use std::any::Any;

/// Lightweight reference to a received network message.
///
/// Captures the fields that event handlers typically inspect: the message
/// command discriminator and the uncompressed payload bytes. The full
/// `Message` type (with compression support) lives in `neo-core`.
#[derive(Debug, Clone)]
pub struct NetworkMessageRef {
    /// Message command discriminator.
    pub command: MessageCommand,
    /// Uncompressed payload bytes.
    pub payload_raw: Vec<u8>,
}

/// Handler of MessageReceived event from RemoteNode.
/// Triggered when a new message is received from a peer.
pub trait MessageReceivedHandler {
    /// Called when a message is received from a remote peer; returns whether it was handled.
    fn remote_node_message_received_handler(
        &self,
        system: &dyn Any,
        message: &NetworkMessageRef,
    ) -> bool;
}
