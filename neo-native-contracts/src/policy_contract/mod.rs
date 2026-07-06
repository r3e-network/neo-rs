//! # neo-native-contracts::policy_contract
//!
//! Native Policy contract fee, account, and storage policy behavior.
//!
//! ## Boundary
//!
//! This module belongs to `neo-native-contracts`. This execution-domain crate
//! owns native contract logic and storage codecs and must not own node startup,
//! RPC transport, or P2P sync.
//!
//! ## Contents
//!
//! - `constants`: Protocol constants, storage prefixes, bounds, and event names.
//! - `initialize`: genesis policy setting seeding.
//! - `invoke`: Native method handlers and runtime side effects.
//! - `metadata`: Native contract metadata and descriptor helpers.
//! - `provider`: Engine-facing blocked-contract and whitelisted-fee seams.
//! - `storage`: Storage contexts, key builders, and storage item helpers for
//!   execution.
//! - `tests`: Module-local tests and regression coverage.

use crate::hashes::POLICY_CONTRACT_HASH;
use neo_error::CoreResult;
use neo_execution::{ApplicationEngine, NativeContract, NativeEvent, NativeMethod};
use neo_primitives::UInt160;
use neo_storage::persistence::DataCache;

mod constants;
mod initialize;
mod invoke;
mod metadata;
mod provider;
mod storage;

pub(in crate::policy_contract) use constants::*;
pub use constants::{
    DEFAULT_EXEC_FEE_FACTOR, DEFAULT_FEE_PER_BYTE, DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT,
};
pub(crate) use constants::{
    POLICY_MILLISECONDS_PER_BLOCK_CHANGED_EVENT, POLICY_RECOVERED_FUND_EVENT,
    POLICY_WHITELIST_FEE_CHANGED_EVENT,
};

native_contract_handle!(
    /// Static accessor for the PolicyContract native contract.
    pub struct PolicyContract {
        id: -7,
        contract_name: "PolicyContract",
        hash: POLICY_CONTRACT_HASH,
    }
);

impl NativeContract for PolicyContract {
    native_contract_identity!(PolicyContract);

    fn methods(&self) -> &[NativeMethod] {
        &metadata::POLICY_CONTRACT_METHODS
    }

    fn supports_empty_block_fast_forward(&self) -> bool {
        true
    }

    fn event_descriptors(&self) -> &[NativeEvent] {
        &metadata::POLICY_CONTRACT_EVENTS
    }

    fn is_contract_blocked(
        &self,
        snapshot: &neo_storage::persistence::DataCache,
        contract_hash: &UInt160,
    ) -> CoreResult<bool> {
        self.is_contract_blocked_native(snapshot, contract_hash)
    }

    fn initialize(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        self.initialize_native(engine)
    }

    fn whitelisted_fee(
        &self,
        snapshot: &DataCache,
        contract_hash: &UInt160,
        method: &str,
        param_count: u32,
    ) -> CoreResult<Option<i64>> {
        self.whitelisted_fee_native(snapshot, contract_hash, method, param_count)
    }

    native_contract_dispatch!(metadata::POLICY_CONTRACT_METHOD_BINDINGS);
}

#[cfg(test)]
#[path = "../tests/policy_contract/mod.rs"]
mod tests;
