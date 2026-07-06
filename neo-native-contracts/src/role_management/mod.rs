//! # neo-native-contracts::role_management
//!
//! Native RoleManagement state and designated-node behavior.
//!
//! ## Boundary
//!
//! This module belongs to `neo-native-contracts`. This execution-domain crate
//! owns native contract logic and storage codecs and must not own node startup,
//! RPC transport, or P2P sync.
//!
//! ## Contents
//!
//! - `invoke`: native method dispatch for designation query and writer calls.
//! - `metadata`: Native contract metadata and descriptor helpers.
//! - `node_list`: designated-node list storage.
//! - `providers`: designated-node provider helpers and event-state builders.
//! - `storage`: Storage contexts, key builders, and storage item helpers for
//!   execution.
//! - `tests`: Module-local tests and regression coverage.

mod invoke;
mod metadata;
mod node_list;
mod providers;
mod storage;

use neo_error::CoreResult;
use neo_execution::{ApplicationEngine, NativeContract, NativeEvent, NativeMethod};

use crate::hashes::ROLE_MANAGEMENT_HASH;

pub(crate) const ROLE_DESIGNATION_EVENT: &str = "Designation";

native_contract_handle!(
    /// The RoleManagement native contract.
    pub struct RoleManagement {
        id: -8,
        contract_name: "RoleManagement",
        hash: ROLE_MANAGEMENT_HASH,
    }
);

impl NativeContract for RoleManagement {
    native_contract_identity!(RoleManagement);

    fn methods(&self) -> &[NativeMethod] {
        &metadata::ROLE_MANAGEMENT_METHODS
    }

    fn supports_empty_block_fast_forward(&self) -> bool {
        true
    }

    fn event_descriptors(&self) -> &[NativeEvent] {
        &metadata::ROLE_MANAGEMENT_EVENTS
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        self.invoke_native(engine, method, args)
    }

    native_contract_resolved_invoke!(metadata::ROLE_MANAGEMENT_METHOD_BINDINGS);
}

#[cfg(test)]
#[path = "../tests/role_management/mod.rs"]
mod tests;
