//! RoleManagement native-method handlers.
//!
//! Keeps designation query and writer bodies out of the contract root while
//! preserving role validation, committee checks, storage layout, duplicate-node
//! rejection, hardfork-gated event state, and current-index bounds. Dispatch is
//! declared by the metadata binding table and `native_contract_dispatch!`.

use super::{ROLE_DESIGNATION_EVENT, RoleManagement, node_list, storage};
use crate::LedgerContract;
use neo_config::Hardfork;
use neo_error::{CoreError, CoreResult};
use neo_execution::ApplicationEngine;
use neo_storage::StorageItem;

impl RoleManagement {
    pub(super) fn invoke_get_designated_by_role(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let context = "RoleManagement::getDesignatedByRole";
        let role_value = crate::args::raw_u32_arg(args, 0, context)
            .map_err(|_| CoreError::invalid_operation("RoleManagement: missing/invalid role"))?;
        // C# validates the role against the Role enum.
        let role_byte = Self::parse_role_arg(role_value)?.as_byte();
        let index = crate::args::raw_u32_arg(args, 1, context)
            .map_err(|_| CoreError::invalid_operation("RoleManagement: missing/invalid index"))?;

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
            // node-list array - exactly the Array return wants.
            Some(value) => Ok(value),
            None => node_list::empty_node_list(),
        }
    }

    pub(super) fn invoke_designate_as_role(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // C# order: validate nodes (1..32) -> validate role ->
        // AssertCommittee -> require persisting block -> reject duplicate
        // -> store sorted -> emit Designation event.
        let context = "RoleManagement::designateAsRole";
        let role_value = crate::args::raw_u32_arg(args, 0, context)
            .map_err(|_| CoreError::invalid_operation("RoleManagement: missing/invalid role"))?;
        let nodes_bytes = args.get(1).ok_or_else(|| {
            CoreError::invalid_operation("RoleManagement: missing nodes argument")
        })?;
        let nodes = node_list::parse_nodes_arg(nodes_bytes)?;
        let role_byte = Self::parse_role_arg(role_value)?.as_byte();

        // C# AssertCommittee.
        crate::committee::assert_committee(engine, "designateAsRole")?;

        // C# v3.10.1 DesignateAsRole: reject a node list containing
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
            .ok_or_else(|| CoreError::invalid_operation("designateAsRole: no persisting block"))?;
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
        let state =
            Self::designation_event_state(role_byte, block_index, echidna, &old_nodes, &nodes)?;
        engine
            .send_notification(
                Self::script_hash(),
                ROLE_DESIGNATION_EVENT.to_owned(),
                state,
            )
            .map_err(|e| CoreError::invalid_operation(format!("designateAsRole notify: {e}")))?;
        Ok(Vec::new())
    }
}
