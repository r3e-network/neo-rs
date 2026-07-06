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
//! - `constants`: native event names, storage prefixes, and genesis defaults.
//! - `initialize`: genesis deployment-setting seeding.
//! - `invoke`: native method dispatch for query and lifecycle calls.
//! - `metadata`: Native contract metadata and descriptor helpers.
//! - `operations`: native-contract operation handlers.
//! - `persist`: native deployment and hardfork-refresh hook.
//! - `tests`: Module-local tests and regression coverage.

use crate::hashes::CONTRACT_MANAGEMENT_HASH;
use neo_error::CoreResult;
use neo_execution::{ApplicationEngine, ContractState, NativeContract, NativeEvent, NativeMethod};
use neo_primitives::UInt160;
use neo_storage::persistence::DataCache;

mod constants;
mod initialize;
mod invoke;
mod metadata;
mod operations;
mod persist;

pub(in crate::contract_management) use constants::*;
pub(crate) use constants::{CONTRACT_DEPLOY_EVENT, CONTRACT_DESTROY_EVENT, CONTRACT_UPDATE_EVENT};

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

    fn initialize(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        self.initialize_native(engine)
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
