//! # neo-native-contracts::contract_management
//!
//! Native ContractManagement state, storage, and lifecycle operations.
//!
//! ## Boundary
//!
//! This module belongs to `neo-native-contracts`. This execution-domain crate
//! owns native contract logic and storage codecs and must not own node startup,
//! RPC transport, or P2P sync.
//!
//! ## Contents
//!
//! - `invoke`: native method dispatch for query and lifecycle calls.
//! - `metadata`: Native contract metadata and descriptor helpers.
//! - `operations`: native-contract operation handlers.
//! - `persist`: native deployment and hardfork-refresh hook.
//! - `tests`: Module-local tests and regression coverage.

use crate::hashes::CONTRACT_MANAGEMENT_HASH;
use neo_error::CoreResult;
use neo_execution::{ApplicationEngine, ContractState, NativeContract, NativeEvent, NativeMethod};
use neo_primitives::UInt160;
use neo_storage::StorageItem;
use neo_storage::persistence::DataCache;
use num_bigint::BigInt;

mod invoke;
mod metadata;
mod operations;
mod persist;

pub(crate) const CONTRACT_DEPLOY_EVENT: &str = "Deploy";
pub(crate) const CONTRACT_UPDATE_EVENT: &str = "Update";
pub(crate) const CONTRACT_DESTROY_EVENT: &str = "Destroy";

/// Storage prefix for the minimum-deployment-fee setting (C#
/// `ContractManagement.Prefix_MinimumDeploymentFee`).
const PREFIX_MINIMUM_DEPLOYMENT_FEE: u8 = 20;
/// C# default minimum deployment fee: 10 GAS, in datoshi.
const DEFAULT_MINIMUM_DEPLOYMENT_FEE: i64 = 10_00000000;

/// Storage prefix for the per-contract record (matches C#
/// `ContractManagement.PREFIX_CONTRACT`).
const PREFIX_CONTRACT: u8 = 8;
/// Storage prefix for the contract-id → hash index (matches C#
/// `ContractManagement.PREFIX_CONTRACT_HASH`).
const PREFIX_CONTRACT_HASH: u8 = 12;
/// Storage prefix for the next-available-contract-id counter (matches C#
/// `ContractManagement.Prefix_NextAvailableId`).
const PREFIX_NEXT_AVAILABLE_ID: u8 = 15;
/// C# genesis value for `Prefix_NextAvailableId` (`InitializeAsync` writes 1).
const DEFAULT_NEXT_AVAILABLE_ID: i64 = 1;

native_contract_handle!(
    /// Static accessor for the ContractManagement native contract.
    pub struct ContractManagement {
        id: -1,
        contract_name: "ContractManagement",
        hash: CONTRACT_MANAGEMENT_HASH,
    }
);

impl NativeContract for ContractManagement {
    native_contract_identity!(ContractManagement);

    fn methods(&self) -> &[NativeMethod] {
        &metadata::CONTRACT_MANAGEMENT_METHODS
    }

    fn supports_empty_block_fast_forward(&self) -> bool {
        true
    }

    fn event_descriptors(&self) -> &[NativeEvent] {
        &metadata::CONTRACT_MANAGEMENT_EVENTS
    }

    /// C# `ContractManagement.InitializeAsync(engine, hardfork)` for `hardfork
    /// == ActiveIn` (ContractManagement.cs:53-61; the contract is
    /// genesis-active, so this runs while persisting block 0): seed
    /// `Prefix_MinimumDeploymentFee` (10 GAS) and `Prefix_NextAvailableId` (1).
    fn initialize(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        let snapshot = engine.snapshot_cache();
        snapshot.add(
            Self::minimum_deployment_fee_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_MINIMUM_DEPLOYMENT_FEE,
            ))),
        );
        snapshot.add(
            Self::next_available_id_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_NEXT_AVAILABLE_ID,
            ))),
        );
        Ok(())
    }

    fn on_persist(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        self.on_persist_native(engine)
    }

    /// Resolves a deployed contract's state from storage.
    ///
    /// ContractManagement owns the per-contract records, so it backs the
    /// engine's `fetch_contract` storage path (via the native-contract
    /// provider seam): `System.Contract.Call` to any deployed contract —
    /// native or user — resolves its NEF/manifest through here. Delegates to
    /// the read helper used by the `getContract` invoke arm.
    fn lookup_contract_state(
        &self,
        snapshot: &DataCache,
        hash: &UInt160,
    ) -> CoreResult<Option<ContractState>> {
        Self::get_contract_from_snapshot(snapshot, hash)
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        self.invoke_native(engine, method, args)
    }
}

#[cfg(test)]
use persist::NATIVE_CONTRACTS;

#[cfg(test)]
#[path = "../tests/contract_management/mod.rs"]
mod tests;
