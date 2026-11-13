//! Neo protocol settings, hardfork activation, and signing data.

pub mod builders;
pub mod call_flags;
mod config;
pub mod execution;
pub mod hardfork;
pub mod header_wire;
pub mod payloads;
pub mod protocol_settings;
pub mod signing_data;
pub mod witness;

pub use hardfork::{Hardfork, HardforkManager, HardforkParseError};
pub use protocol_settings::ProtocolSettings;
pub use signing_data::{SIGN_DATA_LENGTH, get_sign_data, get_sign_data_vec};

// ---------------------------------------------------------------------------
// Crate-root module aliases.
//
// After the `neo-core` decomposition, downstream crates (neo-smart-contract,
// neo-native-contracts, ...) refer to payload types through the former
// `neo-core` crate-root paths (`crate::block`, `crate::signer`, ...). Those
// paths are re-exposed here so `neo_protocol::block::Block` etc. resolve
// without each consumer reaching into `neo_protocol::payloads::*`.
// ---------------------------------------------------------------------------
pub use payloads::block;
pub use payloads::signer;
pub use payloads::transaction;
pub use payloads::transaction_attribute;

/// Witness condition authority (canonical source: `neo_io::witness_rule`).
pub mod witness_condition {
    pub use neo_io::witness_rule::WitnessCondition;
}
/// Witness rule action authority (canonical source: `neo_io::witness_rule`).
pub mod witness_rule_action {
    pub use neo_io::witness_rule::WitnessRuleAction;
}
