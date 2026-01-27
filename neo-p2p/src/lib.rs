//! # Neo P2P
//!
//! Lightweight P2P protocol types for the Neo blockchain.
//!
//! ## Crate Purpose
//!
//! This crate provides **basic P2P protocol types** with minimal dependencies.
//! It is designed for:
//!
//! - **External tools** that need P2P types without pulling in neo-core
//! - **Lightweight applications** that only need message/inventory enums
//! - **Testing** with simple P2P type definitions
//!
//! ## When to Use neo-core::network Instead
//!
//! For full P2P networking functionality, use `neo_core::network::p2p` which provides:
//!
//! - **LocalNode**: Connection management and peer discovery
//! - **RemoteNode**: Per-peer state machine and message handling
//! - **TaskManager**: Coordinated block/transaction synchronization
//! - **Payloads**: Complete message payload implementations
//!
//! ## Architecture Note
//!
//! This crate intentionally provides a subset of types from `neo_core::network::p2p`:
//!
//! - **neo-p2p types**: Basic enums (MessageCommand, InventoryType, etc.)
//! - **neo-core types**: Full-featured implementations with actors, state machines
//!
//! This separation allows neo-p2p to remain dependency-light for external consumers.
//! Neo-core re-exports neo-p2p types via `neo_core::p2p::*`.
//!
//! ## Core Types
//!
//! - [`MessageCommand`]: P2P message command identifiers
//! - [`InventoryType`]: Types of inventory (Transaction, Block, etc.)
//! - [`VerifyResult`]: Transaction/block verification result codes
//! - [`WitnessConditionType`]: Witness condition type identifiers
//! - [`NodeCapabilityType`]: Node capability flags
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
pub mod traits;
pub mod transaction_removal_reason;
pub mod verify_result;
pub mod witness_condition_type;
pub mod witness_rule_action;

// Re-exports
pub use contains_transaction_type::ContainsTransactionType;
pub use error::{P2PError, P2PResult};
pub use inventory_type::InventoryType;
pub use message_command::MessageCommand;
pub use message_flags::MessageFlags;
pub use neo_primitives::{InvalidWitnessScopeError, TransactionAttributeType, WitnessScope};
pub use node_capability_type::NodeCapabilityType;
pub use oracle_response_code::OracleResponseCode;
pub use transaction_removal_reason::TransactionRemovalReason;
pub use verify_result::VerifyResult;
pub use witness_condition_type::WitnessConditionType;
pub use witness_rule_action::WitnessRuleAction;

// P2P traits for implementing network services
pub use traits::{
    Broadcaster, DataRequester, P2PConfig, P2PEvent, P2PEventSubscriber, P2PService, PeerInfo,
    PeerManager,
};
