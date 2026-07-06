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
//! - `storage`: Storage contexts, key builders, and storage item helpers for
//!   execution.
//! - `tests`: Module-local tests and regression coverage.

mod invoke;
mod metadata;
mod node_list;
mod storage;

use neo_crypto::ECPoint;
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, NativeContract, NativeEvent, NativeMethod};
use neo_storage::persistence::DataCache;
use neo_vm::StackItem;

use crate::hashes::ROLE_MANAGEMENT_HASH;
use crate::role::Role;

pub(crate) const ROLE_DESIGNATION_EVENT: &str = "Designation";

native_contract_handle!(
    /// The RoleManagement native contract.
    pub struct RoleManagement {
        id: -8,
        contract_name: "RoleManagement",
        hash: ROLE_MANAGEMENT_HASH,
    }
);

impl RoleManagement {
    /// Looks up the public keys designated for `role`, effective at block
    /// `height` (the most recent designation with index ≤ `height`).
    pub fn get_designated_by_role_at(
        &self,
        snapshot: &DataCache,
        role: Role,
        height: u32,
    ) -> CoreResult<Vec<ECPoint>> {
        match storage::find_designation_value(snapshot, role.as_byte(), height) {
            Some(value) => node_list::decode_node_list(&value),
            None => Ok(Vec::new()),
        }
    }

    /// The designation storage key `(RoleManagement.ID, [role_byte, index_be])`.
    pub(crate) fn designation_key(role_byte: u8, index: u32) -> neo_storage::StorageKey {
        storage::designation_key(role_byte, index)
    }

    /// Parses the VM integer role argument through the shared Neo N3 role mapping.
    fn parse_role_arg(role_value: u32) -> CoreResult<Role> {
        u8::try_from(role_value)
            .ok()
            .and_then(Role::from_byte)
            .ok_or_else(|| {
                CoreError::invalid_operation(format!(
                    "RoleManagement: role {role_value} is not valid"
                ))
            })
    }

    /// Builds the `Designation` event state, mirroring C# `DesignateAsRole`'s
    /// `SendNotification`. Pre-`HF_Echidna` the state is `[role, blockIndex]`; from
    /// `HF_Echidna` it is `[role, blockIndex, oldNodes, newNodes]` where `oldNodes`
    /// is the previously-effective (stored, sorted) list and `newNodes` is the
    /// caller's input list in its original order.
    fn designation_event_state(
        role_byte: u8,
        block_index: u32,
        echidna: bool,
        old_nodes: &[ECPoint],
        new_nodes: &[ECPoint],
    ) -> CoreResult<Vec<StackItem>> {
        let mut state = vec![
            StackItem::from_int(i64::from(role_byte)),
            StackItem::from_int(i64::from(block_index)),
        ];
        if echidna {
            state.push(node_list::nodes_to_event_array(old_nodes)?);
            state.push(node_list::nodes_to_event_array(new_nodes)?);
        }
        Ok(state)
    }
}

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
}

#[cfg(test)]
#[path = "../tests/role_management/mod.rs"]
mod tests;
