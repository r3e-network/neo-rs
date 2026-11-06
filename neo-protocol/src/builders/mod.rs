// Copyright (c) 2024 R3E Network
// Licensed under the MIT License. See LICENSE file for details.

//! Builder helpers for the protocol layer.
//!
//! Exposes the VM [`ScriptBuilder`] used by contract redeem-script construction
//! and witness script helpers, together with fluent builders that produce the
//! canonical protocol payload types (`Transaction`, `Signer`, `Witness`,
//! witness conditions/rules, and transaction attributes).

pub mod script_builder;
mod signer;
mod transaction;
mod transaction_attributes;
mod witness;
mod witness_condition;

pub use script_builder::ScriptBuilder;
pub use signer::SignerBuilder;
pub use transaction::TransactionBuilder;
pub use transaction_attributes::TransactionAttributesBuilder;
pub use witness::WitnessBuilder;
pub use witness_condition::{
    AndConditionBuilder, OrConditionBuilder, WitnessConditionBuilder, WitnessRuleBuilder,
};
