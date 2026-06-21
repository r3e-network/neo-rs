//! RoleManagement native contract (id -8).
//!
//! Implements `getDesignatedByRole` of the C#
//! `Neo.SmartContract.Native.RoleManagement`: the public keys designated for a
//! role (StateValidator / Oracle / NeoFSAlphabetNode / P2PNotary), effective at
//! a given block index. The designation that applies is the most recent one
//! whose designation index is ≤ the queried index — C# performs a backward range
//! seek between `(role, index)` and the `(role)` boundary; this module replicates
//! it with a descending prefix scan. The committee-gated `designateAsRole`
//! writer lives in the runtime, which writes the entries this module reads.

mod metadata;
mod node_list;
mod storage;

use neo_config::Hardfork;
use neo_crypto::ECPoint;
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, NativeContract, NativeEvent, NativeMethod};
use neo_storage::StorageItem;
use neo_storage::persistence::DataCache;
use neo_vm::StackItem;

use crate::LedgerContract;
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

    fn event_descriptors(&self) -> &[NativeEvent] {
        &metadata::ROLE_MANAGEMENT_EVENTS
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        match method {
            "getDesignatedByRole" => {
                let role_value =
                    crate::args::raw_u32_arg(args, 0, "RoleManagement::getDesignatedByRole")
                        .map_err(|_| {
                            CoreError::invalid_operation("RoleManagement: missing/invalid role")
                        })?;
                // C# validates the role against the Role enum.
                let role_byte = Self::parse_role_arg(role_value)?.as_byte();
                let index =
                    crate::args::raw_u32_arg(args, 1, "RoleManagement::getDesignatedByRole")
                        .map_err(|_| {
                            CoreError::invalid_operation("RoleManagement: missing/invalid index")
                        })?;

                let snapshot = engine.snapshot_cache();
                // C# throws when index > currentIndex + 1.
                let current = LedgerContract::new().current_index(&snapshot)?;
                if current.saturating_add(1) < index {
                    return Err(CoreError::invalid_operation(format!(
                        "RoleManagement: index {index} exceeds current index + 1 ({})",
                        current.saturating_add(1)
                    )));
                }

                match storage::find_designation_value(&snapshot, role_byte, index) {
                    // The stored value is already the BinarySerializer-encoded
                    // node-list array — exactly the Array return wants.
                    Some(value) => Ok(value),
                    None => node_list::empty_node_list(),
                }
            }
            "designateAsRole" => {
                // C# order: validate nodes (1..32) -> validate role ->
                // AssertCommittee -> require persisting block -> reject duplicate
                // -> store sorted -> emit Designation event.
                let role_value =
                    crate::args::raw_u32_arg(args, 0, "RoleManagement::designateAsRole").map_err(
                        |_| CoreError::invalid_operation("RoleManagement: missing/invalid role"),
                    )?;
                let nodes_bytes = args.get(1).ok_or_else(|| {
                    CoreError::invalid_operation("RoleManagement: missing nodes argument")
                })?;
                let nodes = node_list::parse_nodes_arg(nodes_bytes)?;
                let role_byte = Self::parse_role_arg(role_value)?.as_byte();

                // C# AssertCommittee.
                crate::committee::assert_committee(engine, "designateAsRole")?;

                // C# v3.10.0 DesignateAsRole: reject a node list containing
                // duplicate public keys (`nodes.Distinct().Count() != nodes.Length`).
                let mut deduplicated = nodes.clone();
                deduplicated.sort();
                deduplicated.dedup();
                if deduplicated.len() != nodes.len() {
                    return Err(CoreError::invalid_operation(
                        "Duplicate publickeys are not allowed",
                    ));
                }

                // C#: index = PersistingBlock.Index + 1 (key); the event carries
                // PersistingBlock.Index itself.
                let block_index = engine
                    .persisting_block()
                    .map(|block| block.index())
                    .ok_or_else(|| {
                        CoreError::invalid_operation("designateAsRole: no persisting block")
                    })?;
                let index = block_index.checked_add(1).ok_or_else(|| {
                    CoreError::invalid_operation("designateAsRole: designation index overflow")
                })?;

                let snapshot = engine.snapshot_cache();
                let key = Self::designation_key(role_byte, index);
                if snapshot.get(&key).is_some() {
                    return Err(CoreError::invalid_operation(
                        "designateAsRole: role already designated at this index",
                    ));
                }
                snapshot.add(
                    key,
                    StorageItem::from_bytes(node_list::encode_node_list(&nodes)?),
                );

                // Emit the Designation event; from HF_Echidna it also carries the
                // previously-effective (at block_index) and new node lists.
                let echidna = engine.is_hardfork_enabled(Hardfork::HfEchidna);
                let old_nodes = if echidna {
                    match storage::find_designation_value(&snapshot, role_byte, block_index) {
                        Some(value) => node_list::decode_node_list(&value)?,
                        None => Vec::new(),
                    }
                } else {
                    Vec::new()
                };
                let state = Self::designation_event_state(
                    role_byte,
                    block_index,
                    echidna,
                    &old_nodes,
                    &nodes,
                )?;
                engine
                    .send_notification(
                        Self::script_hash(),
                        ROLE_DESIGNATION_EVENT.to_owned(),
                        state,
                    )
                    .map_err(|e| {
                        CoreError::invalid_operation(format!("designateAsRole notify: {e}"))
                    })?;
                Ok(Vec::new())
            }
            other => Err(CoreError::invalid_operation(format!(
                "RoleManagement method '{other}' is not implemented"
            ))),
        }
    }
}

#[cfg(test)]
#[path = "tests/role_management.rs"]
mod tests;
