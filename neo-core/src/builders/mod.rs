//! Minimal builder helpers mirroring the C# `Neo.Builder` utilities used by
//! the test suite. These builders intentionally expose only the functionality
//! currently required by the tests while keeping the API ergonomic for future
//! extensions.

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
