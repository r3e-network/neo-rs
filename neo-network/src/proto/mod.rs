//! # neo-network::proto
//!
//! Protocol message definitions and network payload framing.
//!
//! ## Boundary
//!
//! This module belongs to `neo-network`. This service crate owns P2P transport
//! and peer behavior and must not execute blocks, own consensus rules, or
//! mutate storage directly.
//!
//! ## Contents
//!
//! - `channels_config`: P2P channel configuration records.
//! - `error`: Typed error definitions and conversions.
//! - `inventory_type`: P2P inventory-type identifiers.
//! - `message_command`: P2P message command identifiers.
//! - `message_flags`: P2P message flag records.

/// Channel configuration for P2P node bootstrap.
pub mod channels_config;

/// Transaction containment type enumeration.
pub mod contains_transaction_type {
    pub use neo_primitives::ContainsTransactionType;
}

/// Low-level P2P protocol error types and result alias.
pub mod error;

/// Inventory type enumeration (Block, Transaction, etc.) and its
/// `From<InventoryType> for MessageCommand` conversion.
pub mod inventory_type;

/// P2P message command identifiers.
pub mod message_command;

/// Message header flags.
pub mod message_flags;

/// Node capability type enumeration.
pub mod node_capability_type {
    pub use neo_primitives::NodeCapabilityType;
}

/// Oracle response code enumeration.
pub mod oracle_response_code {
    pub use neo_primitives::OracleResponseCode;
}

/// Transaction removal reason enumeration.
pub mod transaction_removal_reason {
    pub use neo_primitives::TransactionRemovalReason;
}

/// Verification result enumeration.
pub mod verify_result {
    pub use neo_primitives::VerifyResult;
}

/// Witness condition type enumeration.
pub mod witness_condition_type {
    pub use neo_primitives::WitnessConditionType;
}

/// Witness rule action enumeration.
pub mod witness_rule_action {
    pub use neo_primitives::WitnessRuleAction;
}

// ---------------------------------------------------------------------------
// Public re-exports for callers that need the P2P primitive surface.
// ---------------------------------------------------------------------------

pub use channels_config::ChannelsConfig;
pub use contains_transaction_type::ContainsTransactionType;
pub use error::{P2PError, P2PResult};
pub use inventory_type::InventoryType;
pub use message_command::MessageCommand;
pub use message_flags::MessageFlags;
pub use node_capability_type::NodeCapabilityType;
pub use oracle_response_code::OracleResponseCode;
pub use transaction_removal_reason::TransactionRemovalReason;
pub use verify_result::VerifyResult;
pub use witness_condition_type::WitnessConditionType;
pub use witness_rule_action::WitnessRuleAction;

// Re-exports from neo-primitives (kept for parity with the old crate).
pub use neo_primitives::{InvalidWitnessScopeError, TransactionAttributeType, WitnessScope};
