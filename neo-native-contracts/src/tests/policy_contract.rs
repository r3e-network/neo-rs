//! Tests for the PolicyContract native contract.
//!
//! Extracted from `policy_contract.rs` to keep the production module
//! focused. The `use super::*;` below re-exports the production items so
//! the inner test modules' own `use super::*;` resolves to them.

use super::*;
use neo_primitives::{CallFlags, ContractParameterType, TransactionAttributeType};
use neo_serialization::BinarySerializer;
use neo_storage::StorageKey;
use neo_vm_rs::{ExecutionEngineLimits, StackValue};
use num_traits::ToPrimitive;

#[cfg(test)]
#[path = "policy_contract/policy_writer_tests/mod.rs"]
mod policy_writer_tests;
#[cfg(test)]
#[path = "policy_contract/tests.rs"]
mod tests;
