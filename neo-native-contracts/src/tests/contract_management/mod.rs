//! # neo-native-contracts::tests::contract_management
//!
//! Test module grouping Native ContractManagement state, storage, and lifecycle
//! operations. coverage for neo-native-contracts.
//!
//! ## Boundary
//!
//! This is test/benchmark-only code for neo-native-contracts; it may assemble
//! fixtures but must not introduce production behavior.
//!
//! ## Contents
//!
//! - `deploy_update_engine_tests`: deploy update engine tests types and
//!   helpers.
//! - `destroy_engine_tests`: contract destroy execution coverage.
//! - `persist_tests`: contract persistence coverage.
//! - `tests`: Module-local tests and regression coverage.

use super::*;
use neo_manifest::manifest::contract_manifest::MAX_MANIFEST_LENGTH;
use neo_manifest::{ContractAbi, ContractManifest, NefFile};
use neo_serialization::BinarySerializer;
use neo_storage::StorageKey;
use neo_vm_rs::ExecutionEngineLimits;

fn storage_key_int(snapshot: &DataCache, key: StorageKey) -> Option<BigInt> {
    snapshot
        .get(&key)
        .map(|item| BigInt::from_signed_bytes_le(&item.value_bytes()))
}

#[cfg(test)]
#[path = "deploy_update_engine_tests/mod.rs"]
mod deploy_update_engine_tests;
#[cfg(test)]
#[path = "destroy_engine_tests.rs"]
mod destroy_engine_tests;
#[cfg(test)]
#[path = "persist_tests.rs"]
mod persist_tests;
#[cfg(test)]
#[path = "tests.rs"]
mod tests;
