//! Tests for the NeoToken native contract.
//!
//! Extracted from `neo_token.rs` to keep the production module focused.
//! The `use super::*;` below re-exports the production items
//! (`NeoToken`, `NeoAccountStateView`, `CandidateState`, etc.) so the
//! inner test modules' own `use super::*;` resolves to them.

use super::*;
use neo_primitives::{CallFlags, ContractParameterType};
use neo_serialization::BinarySerializer;
use neo_vm_rs::ExecutionEngineLimits;

#[cfg(test)]
#[path = "neo_token/basic_tests.rs"]
mod basic_tests;
#[cfg(test)]
#[path = "neo_token/committee_recompute_tests.rs"]
mod committee_recompute_tests;
#[cfg(test)]
#[path = "neo_token/governance_writer_tests/mod.rs"]
mod governance_writer_tests;
#[cfg(test)]
#[path = "neo_token/persist_hook_tests.rs"]
mod persist_hook_tests;
#[cfg(test)]
#[path = "neo_token/storage_codec_tests.rs"]
mod storage_codec_tests;
#[cfg(test)]
#[path = "neo_token/witness_harness_tests.rs"]
mod witness_harness_tests;
