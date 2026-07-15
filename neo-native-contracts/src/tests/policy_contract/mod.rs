//! # neo-native-contracts::tests::policy_contract
//!
//! Test module grouping Native Policy contract fee, account, and storage policy
//! behavior. coverage for neo-native-contracts.
//!
//! ## Boundary
//!
//! This is test/benchmark-only code for neo-native-contracts; it may assemble
//! fixtures but must not introduce production behavior.
//!
//! ## Contents
//!
//! - `policy_writer_tests`: policy writer coverage.
//! - `tests`: Module-local tests and regression coverage.

use super::storage::WhitelistedContractView;
use super::*;
use neo_config::Hardfork;
use neo_primitives::{CallFlags, ContractParameterType, TransactionAttributeType};
use neo_serialization::BinarySerializer;
use neo_storage::StorageKey;
use neo_vm::{ExecutionEngineLimits, StackItem};
use num_traits::ToPrimitive;

#[cfg(test)]
#[path = "policy_writer_tests/mod.rs"]
mod policy_writer_tests;
#[cfg(test)]
#[path = "tests.rs"]
mod tests;
