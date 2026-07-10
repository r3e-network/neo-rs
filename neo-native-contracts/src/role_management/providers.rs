//! RoleManagement provider helpers.
//!
//! Keeps designation lookup, key construction, VM role parsing, and notification
//! state assembly out of the contract root while preserving the exact Neo N3
//! storage layout and hardfork-gated event shape.

use super::{RoleManagement, node_list, storage};
use crate::role::Role;
use neo_crypto::ECPoint;
use neo_error::{CoreError, CoreResult};
use neo_storage::CacheRead;
use neo_storage::persistence::DataCache;
use neo_vm::StackItem;

impl RoleManagement {
    /// Looks up the public keys designated for `role`, effective at block
    /// `height` (the most recent designation with index <= `height`).
    pub fn get_designated_by_role_at<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
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
    pub(super) fn parse_role_arg(role_value: u32) -> CoreResult<Role> {
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
    pub(super) fn designation_event_state(
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
