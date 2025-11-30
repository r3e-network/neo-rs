//! # Neo P2P
//!
//! Peer-to-peer networking for the Neo blockchain.
//!
//! This crate provides the P2P networking layer for Neo nodes, including:
//! - Peer discovery and connection management
//! - Message serialization and routing
//! - Block and transaction propagation
//! - Inventory management
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────┐
//! │              neo-p2p                     │
//! │  ┌─────────────────────────────────────┐│
//! │  │         LocalNode                   ││
//! │  │  (Connection management)            ││
//! │  └─────────────────────────────────────┘│
//! │  ┌─────────────────────────────────────┐│
//! │  │         RemoteNode                  ││
//! │  │  (Per-peer state machine)           ││
//! │  └─────────────────────────────────────┘│
//! │  ┌─────────────────────────────────────┐│
//! │  │         Message Types               ││
//! │  │  (Version, Inv, GetData, etc.)      ││
//! │  └─────────────────────────────────────┘│
//! └─────────────────────────────────────────┘
//! ```
//!
//! ## Core Types
//!
//! - [`MessageCommand`]: P2P message command identifiers
//! - [`InventoryType`]: Types of inventory (Transaction, Block, etc.)
//!
//! ## Example
//!
//! ```rust
//! use neo_p2p::{MessageCommand, InventoryType};
//!
//! // Parse message command from byte
//! let cmd = MessageCommand::from_byte(0x2b);
//! assert_eq!(cmd, MessageCommand::Transaction);
//!
//! // Convert inventory type to message command
//! let inv = InventoryType::Block;
//! let cmd: MessageCommand = inv.into();
//! assert_eq!(cmd, MessageCommand::Block);
//! ```

pub mod contains_transaction_type;
pub mod error;
pub mod inventory_type;
pub mod message_command;
pub mod message_flags;
pub mod node_capability_type;
pub mod oracle_response_code;
pub mod transaction_attribute_type;
pub mod transaction_removal_reason;
pub mod verify_result;
pub mod witness_condition_type;
pub mod witness_rule_action;
pub mod witness_scope;

// Re-exports
pub use contains_transaction_type::ContainsTransactionType;
pub use error::{P2PError, P2PResult};
pub use inventory_type::InventoryType;
pub use message_command::MessageCommand;
pub use message_flags::MessageFlags;
pub use node_capability_type::NodeCapabilityType;
pub use oracle_response_code::OracleResponseCode;
pub use transaction_attribute_type::TransactionAttributeType;
pub use transaction_removal_reason::TransactionRemovalReason;
pub use verify_result::VerifyResult;
pub use witness_condition_type::WitnessConditionType;
pub use witness_rule_action::WitnessRuleAction;
pub use witness_scope::WitnessScope;

// Placeholder for future modules
// pub mod local_node;
// pub mod remote_node;
// pub mod message;
// pub mod payloads;
