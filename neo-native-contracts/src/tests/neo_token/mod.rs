//! # neo-native-contracts::tests::neo_token
//!
//! Test module grouping Native NEO token governance, voting, and committee
//! behavior. coverage for neo-native-contracts.
//!
//! ## Boundary
//!
//! This is test/benchmark-only code for neo-native-contracts; it may assemble
//! fixtures but must not introduce production behavior.
//!
//! ## Contents
//!
//! - `basic_tests`: basic token behavior coverage.
//! - `committee_recompute_tests`: committee recomputation coverage.
//! - `governance_writer_tests`: governance writer coverage.
//! - `persist_hook_tests`: persist-hook coverage.
//! - `storage_codec_tests`: storage codec coverage.
//! - `witness_harness_tests`: witness harness coverage.

use super::*;
use neo_primitives::{CallFlags, ContractParameterType};
use neo_serialization::BinarySerializer;
use neo_vm::ExecutionEngineLimits;

#[cfg(test)]
#[path = "basic_tests.rs"]
mod basic_tests;
#[cfg(test)]
#[path = "committee_recompute_tests.rs"]
mod committee_recompute_tests;
#[cfg(test)]
#[path = "governance_writer_tests/mod.rs"]
mod governance_writer_tests;
#[cfg(test)]
#[path = "persist_hook_tests.rs"]
mod persist_hook_tests;
#[cfg(test)]
#[path = "storage_codec_tests.rs"]
mod storage_codec_tests;
#[cfg(test)]
#[path = "witness_harness_tests.rs"]
mod witness_harness_tests;
