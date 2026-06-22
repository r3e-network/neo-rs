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
mod basic_tests;
#[cfg(test)]
mod committee_recompute_tests;
#[cfg(test)]
mod governance_writer_tests;
#[cfg(test)]
mod persist_hook_tests;
#[cfg(test)]
mod storage_codec_tests;
#[cfg(test)]
mod witness_harness_tests;
