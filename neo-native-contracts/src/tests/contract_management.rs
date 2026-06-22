//! Tests for the ContractManagement native contract.
//!
//! Extracted from `contract_management.rs` to keep the production module
//! focused. The `use super::*;` below re-exports the production items so
//! the inner test modules' own `use super::*;` resolves to them.

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
#[path = "contract_management/deploy_update_engine_tests/mod.rs"]
mod deploy_update_engine_tests;
#[cfg(test)]
#[path = "contract_management/destroy_engine_tests.rs"]
mod destroy_engine_tests;
#[cfg(test)]
#[path = "contract_management/persist_tests.rs"]
mod persist_tests;
#[cfg(test)]
#[path = "contract_management/tests.rs"]
mod tests;
