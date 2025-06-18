//! Network message types and protocol handling.
//!
//! This module defines all network message types used in the Neo P2P protocol,
//! following C# Neo's message structure exactly:
//! - commands: Message command definitions (matches C# MessageCommand)
//! - header: Message header structure (matches C# MessageHeader)
//! - inventory: Inventory item types (matches C# InventoryItem)
//! - protocol: Protocol message payloads (matches C# protocol messages)
//! - network: Complete network message wrapper (matches C# NetworkMessage)

pub mod commands;
pub mod header;
pub mod inventory;
pub mod protocol;
pub mod network;
pub mod validation;

// Re-export main types for compatibility
pub use commands::{MessageCommand, MessageType};
pub use header::MessageHeader;
pub use inventory::{InventoryItem, InventoryType};
pub use protocol::ProtocolMessage;
pub use network::NetworkMessage;
pub use validation::MessageValidator;

/// Maximum message payload size (1MB)
pub const MAX_MESSAGE_SIZE: usize = 1_048_576; 