//! P2P protocol primitives — message commands, flags, channels config,
//! timeouts, and the low-level protocol error vocabulary.
//!
//! These types were previously in the standalone `neo-p2p` crate, which has
//! been folded into `neo-network` (it was almost entirely a re-export shim
//! over `neo-primitives`). They are collected here under a `proto` submodule
//! and re-exported from the crate root for backwards compatibility.

/// Channel configuration for P2P node bootstrap.
pub mod channels_config;

/// Transaction containment type enumeration.
pub mod contains_transaction_type {
    //! Re-exported from `neo-primitives`.
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
    //! Re-exported from `neo-primitives`.
    pub use neo_primitives::NodeCapabilityType;
}

/// Oracle response code enumeration.
pub mod oracle_response_code {
    //! Re-exported from `neo-primitives`.
    pub use neo_primitives::OracleResponseCode;
}

/// Shared timeout counters for P2P operations.
pub mod timeouts;

/// Transaction removal reason enumeration.
pub mod transaction_removal_reason {
    //! Re-exported from `neo-primitives`.
    pub use neo_primitives::TransactionRemovalReason;
}

/// Verification result enumeration.
pub mod verify_result {
    //! Re-exported from `neo-primitives`.
    pub use neo_primitives::VerifyResult;
}

/// Witness condition type enumeration.
pub mod witness_condition_type {
    //! Re-exported from `neo-primitives`.
    pub use neo_primitives::WitnessConditionType;
}

/// Witness rule action enumeration.
pub mod witness_rule_action {
    //! Re-exported from `neo-primitives`.
    pub use neo_primitives::WitnessRuleAction;
}

// ---------------------------------------------------------------------------
// Public re-exports (mirror the old `neo_p2p` surface).
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
