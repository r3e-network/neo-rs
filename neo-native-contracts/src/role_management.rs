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
mod tests {
    use super::node_list::{self, NodeList};
    use super::storage;
    use super::*;
    use neo_primitives::{CallFlags, ContractParameterType};
    use neo_serialization::BinarySerializer;
    use neo_storage::StorageItem;
    use neo_vm::Interoperable;
    use neo_vm_rs::StackValue;

    fn sample_point() -> ECPoint {
        // A genesis-validator public key (valid compressed EC point).
        ECPoint::from_bytes(&hex_to_bytes(
            "03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c",
        ))
        .unwrap()
    }

    fn hex_to_bytes(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn native_contract_surface() {
        let c = RoleManagement::new();
        let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["getDesignatedByRole", "designateAsRole"]);
        // The writer is non-safe with write + notify flags and a Void return.
        let d = c
            .methods()
            .iter()
            .find(|m| m.name == "designateAsRole")
            .unwrap();
        assert!(!d.safe);
        assert_eq!(
            d.required_call_flags,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits()
        );
        assert_eq!(
            d.parameters,
            vec![ContractParameterType::Integer, ContractParameterType::Array]
        );
        assert_eq!(d.return_type, ContractParameterType::Void);
    }

    #[test]
    fn designation_storage_key_helpers_match_csharp_layout() {
        let role = Role::Oracle.as_byte();
        let prefix = storage::designation_prefix_key(role);
        assert_eq!(prefix.id(), RoleManagement::ID);
        assert_eq!(prefix.suffix(), &[role]);

        let key = RoleManagement::designation_key(role, 0x0102_0304);
        assert_eq!(key.id(), RoleManagement::ID);
        assert_eq!(key.suffix(), &[role, 1, 2, 3, 4]);
        assert!(key.suffix().starts_with(prefix.suffix()));
    }

    #[test]
    fn parse_role_arg_uses_shared_role_mapping() {
        for role in [
            Role::StateValidator,
            Role::Oracle,
            Role::NeoFsAlphabetNode,
            Role::P2PNotary,
        ] {
            assert_eq!(
                RoleManagement::parse_role_arg(u32::from(role.as_byte())).unwrap(),
                role
            );
        }

        assert!(RoleManagement::parse_role_arg(0).is_err());
        assert!(RoleManagement::parse_role_arg(5).is_err());
        assert!(RoleManagement::parse_role_arg(256).is_err());
    }

    #[test]
    fn invoke_role_integer_args_use_shared_raw_parser() {
        let source = include_str!("role_management.rs");
        let start = source
            .find("fn invoke(")
            .expect("RoleManagement invoke exists");
        let end = source[start..]
            .find("other => Err")
            .map(|offset| start + offset)
            .expect("invoke default arm exists");
        let invoke = &source[start..end];

        assert!(invoke.contains("crate::args::raw_u32_arg"));
        assert!(!invoke.contains("BigInt::from_signed_bytes_le(args"));
        assert!(!invoke.contains("BigInt::from_signed_bytes_le(b)"));
    }

    #[test]
    fn encode_node_list_sorts_and_round_trips() {
        // Two distinct valid points, given in non-sorted order.
        let a = sample_point();
        let b = ECPoint::from_bytes(&hex_to_bytes(
            "02df48f60e8f3e01c48ff40b9b7f1310d7a8b2a193188befe1c2e3df740e895093",
        ))
        .unwrap();
        let input = vec![a.clone(), b.clone()];
        // encode_node_list stores them sorted; decode_node_list reads them back.
        let encoded = node_list::encode_node_list(&input).unwrap();
        let mut expected = input.clone();
        expected.sort();
        let expected_value = StackValue::Array(
            0,
            expected
                .iter()
                .map(|point| StackValue::ByteString(point.to_bytes()))
                .collect(),
        );
        let expected_encoded = BinarySerializer::serialize_stack_value_default(&expected_value)
            .expect("expected node-list StackValue serializes");
        assert_eq!(encoded, expected_encoded);
        let decoded = node_list::decode_node_list(&encoded).unwrap();
        assert_eq!(decoded, expected);
    }

    #[test]
    fn node_list_interoperable_projection_matches_csharp_shape() {
        let a = sample_point();
        let b = ECPoint::from_bytes(&hex_to_bytes(
            "02df48f60e8f3e01c48ff40b9b7f1310d7a8b2a193188befe1c2e3df740e895093",
        ))
        .unwrap();
        let nodes = vec![a.clone(), b.clone()];
        let state = NodeList::new(nodes.clone());
        let expected_value = StackValue::Array(
            0,
            vec![
                StackValue::ByteString(a.to_bytes()),
                StackValue::ByteString(b.to_bytes()),
            ],
        );

        let trait_value = Interoperable::to_stack_value(&state).unwrap();
        assert_eq!(trait_value, expected_value);

        let mut parsed = NodeList::new(Vec::new());
        Interoperable::from_stack_value(&mut parsed, trait_value).unwrap();
        assert_eq!(parsed.into_nodes(), nodes);
    }

    #[test]
    fn node_list_storage_codecs_use_stack_value_projection() {
        fn slice_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
            let start_index = source.find(start).expect("start marker exists");
            let end_index = source[start_index..]
                .find(end)
                .map(|offset| start_index + offset)
                .expect("end marker exists");
            &source[start_index..end_index]
        }

        let source = include_str!("role_management/node_list.rs");
        let decoder = slice_between(source, "fn decode_node_list", "fn empty_node_list");
        assert!(decoder.contains("deserialize_stack_value_with_limits"));
        assert!(decoder.contains("NodeList::from_stack_value"));
        assert!(!decoder.contains("StackValue::Array"));

        let empty_encoder = slice_between(
            source,
            "fn empty_node_list",
            "/// Builds the persisted `StackValue::Array`",
        );
        assert!(empty_encoder.contains("NodeList::new"));
        assert!(empty_encoder.contains("to_stack_value"));
        assert!(empty_encoder.contains("serialize_stack_value_default"));
        assert!(!empty_encoder.contains("StackValue::Array"));

        let projector = slice_between(source, "fn nodes_to_stack_value", "fn nodes_to_event_array");
        assert!(projector.contains("NodeList::new"));
        assert!(projector.contains("to_stack_value"));
        assert!(!projector.contains("StackValue::Array"));
    }

    #[test]
    fn parse_nodes_arg_enforces_1_to_32() {
        // Empty array -> rejected.
        let empty =
            BinarySerializer::serialize_stack_value_default(&StackValue::Array(0, Vec::new()))
                .unwrap();
        assert!(node_list::parse_nodes_arg(&empty).is_err());
        // One valid node -> accepted.
        let one = node_list::encode_node_list(&[sample_point()]).unwrap();
        assert_eq!(node_list::parse_nodes_arg(&one).unwrap().len(), 1);
    }

    #[test]
    fn designation_event_state_shape_matches_hardfork() {
        let new_nodes = vec![sample_point()];
        let old_nodes: Vec<ECPoint> = Vec::new();

        // Pre-Echidna: [role, blockIndex].
        let pre =
            RoleManagement::designation_event_state(8, 41, false, &old_nodes, &new_nodes).unwrap();
        assert_eq!(pre.len(), 2);
        assert_eq!(pre[0].as_int().unwrap(), num_bigint::BigInt::from(8));
        assert_eq!(pre[1].as_int().unwrap(), num_bigint::BigInt::from(41));

        // Echidna: [role, blockIndex, oldNodes(Array), newNodes(Array)].
        let post =
            RoleManagement::designation_event_state(8, 41, true, &old_nodes, &new_nodes).unwrap();
        assert_eq!(post.len(), 4);
        assert!(matches!(post[2], StackItem::Array(_)));
        match &post[3] {
            StackItem::Array(arr) => assert_eq!(arr.items().len(), 1),
            other => panic!("expected newNodes Array, got {other:?}"),
        }
    }

    #[test]
    fn designation_backward_seek_picks_most_recent() {
        let cache = DataCache::new(false);
        let point = sample_point();

        // No designation yet -> empty.
        assert!(
            RoleManagement::new()
                .get_designated_by_role_at(&cache, Role::Oracle, 100)
                .unwrap()
                .is_empty()
        );

        // Designate the Oracle role at index 10 (a 1-element node list).
        let list = BinarySerializer::serialize_stack_value_default(&StackValue::Array(
            0,
            vec![StackValue::ByteString(point.to_bytes())],
        ))
        .unwrap();
        cache.add(
            RoleManagement::designation_key(Role::Oracle.as_byte(), 10),
            StorageItem::from_bytes(list),
        );

        // Before the designation -> still empty; at/after -> the designated key.
        assert!(
            RoleManagement::new()
                .get_designated_by_role_at(&cache, Role::Oracle, 9)
                .unwrap()
                .is_empty()
        );
        let got = RoleManagement::new()
            .get_designated_by_role_at(&cache, Role::Oracle, 25)
            .unwrap();
        assert_eq!(got, vec![point.clone()]);

        // A different role is unaffected.
        assert!(
            RoleManagement::new()
                .get_designated_by_role_at(&cache, Role::StateValidator, 25)
                .unwrap()
                .is_empty()
        );
    }
}
