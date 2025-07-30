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
pub mod network;
pub mod protocol;
pub mod validation;

pub use commands::{varlen, MessageCommand, MessageFlags};
pub use header::{Neo3Message, MAX_MESSAGE_SIZE};
pub use inventory::{InventoryItem, InventoryType};
pub use network::NetworkMessage;
pub use protocol::ProtocolMessage;
pub use validation::MessageValidator;
