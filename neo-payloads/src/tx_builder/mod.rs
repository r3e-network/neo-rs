//! # neo-payloads::tx_builder
//!
//! Transaction builder helpers for constructing Neo payloads.
//!
//! ## Boundary
//!
//! This module belongs to `neo-payloads`. This protocol crate owns payload
//! records and validation helpers and must not perform IO, storage commits, or
//! service orchestration.
//!
//! ## Contents
//!
//! - `signer`: signer configuration and signing helpers.
//! - `transaction`: Transaction body, signer, witness, and fee records.
//! - `transaction_attributes`: transaction attribute builder helpers.
//! - `witness`: witness records and serialization helpers.
//! - `witness_condition`: witness condition builder helpers.

mod signer;
mod transaction;
mod transaction_attributes;
mod witness;
mod witness_condition;

pub use signer::SignerBuilder;
pub use transaction::TransactionBuilder;
pub use transaction_attributes::TransactionAttributesBuilder;
pub use witness::WitnessBuilder;
pub use witness_condition::{
    AndConditionBuilder, OrConditionBuilder, WitnessConditionBuilder, WitnessRuleBuilder,
};
