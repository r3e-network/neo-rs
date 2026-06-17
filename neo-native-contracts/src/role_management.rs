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

use std::any::Any;
use std::sync::LazyLock;

use neo_config::Hardfork;
use neo_crypto::ECPoint;
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, NativeContract, NativeEvent, NativeMethod};
use neo_primitives::{CallFlags, ContractParameterType, UInt160};
use neo_serialization::BinarySerializer;
use neo_storage::persistence::{DataCache, SeekDirection};
use neo_storage::{StorageItem, StorageKey};
use neo_vm::{Interoperable, StackItem};
use neo_vm_rs::{ExecutionEngineLimits, StackValue};
use num_bigint::BigInt;
use num_traits::ToPrimitive;

use crate::LedgerContract;
use crate::hashes::ROLE_MANAGEMENT_HASH;
use crate::role::Role;

/// The RoleManagement native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct RoleManagement;

impl RoleManagement {
    /// Stable native contract id (matches C# `RoleManagement`).
    pub const ID: i32 = -8;
    /// Stable native contract name (matches C# `RoleManagement.Name`).
    pub const NAME: &'static str = "RoleManagement";

    /// Construct a new `RoleManagement` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the RoleManagement script hash.
    pub fn hash(&self) -> UInt160 {
        Self::script_hash()
    }

    /// Returns the RoleManagement script hash.
    pub fn script_hash() -> UInt160 {
        *ROLE_MANAGEMENT_HASH
    }

    /// Looks up the public keys designated for `role`, effective at block
    /// `height` (the most recent designation with index ≤ `height`).
    pub fn get_designated_by_role_at(
        &self,
        snapshot: &DataCache,
        role: Role,
        height: u32,
    ) -> CoreResult<Vec<ECPoint>> {
        match self.find_designation_value(snapshot, role.as_byte(), height) {
            Some(value) => Self::decode_node_list(&value),
            None => Ok(Vec::new()),
        }
    }

    /// Finds the serialized node-list value for `role_byte` effective at `index`:
    /// the entry with the greatest designation index that is ≤ `index`.
    ///
    /// Designations are stored under key `(RoleManagement.ID, [role_byte, index_be])`.
    /// A `Backward` prefix scan yields them in descending designation-index order, so
    /// the first one with `designation_index <= index` is the effective designation —
    /// equivalent to C#'s `FindRange((role, index), (role), Backward).FirstOrDefault`.
    fn find_designation_value(
        &self,
        snapshot: &DataCache,
        role_byte: u8,
        index: u32,
    ) -> Option<Vec<u8>> {
        let prefix = StorageKey::new(RoleManagement::ID, vec![role_byte]);
        for (key, item) in snapshot.find(Some(&prefix), SeekDirection::Backward) {
            let key_bytes = key.key();
            if key_bytes.len() >= 5 {
                let designation_index =
                    u32::from_be_bytes([key_bytes[1], key_bytes[2], key_bytes[3], key_bytes[4]]);
                if designation_index <= index {
                    return Some(item.value_bytes().into_owned());
                }
            }
        }
        None
    }

    /// Decodes a serialized node-list (a `BinarySerializer` array of compressed
    /// EC-point byte strings) into `ECPoint`s.
    fn decode_node_list(value: &[u8]) -> CoreResult<Vec<ECPoint>> {
        let limits = ExecutionEngineLimits::default();
        let value = BinarySerializer::deserialize_stack_value_with_limits(
            value,
            limits.max_item_size as usize,
            limits.max_stack_size as usize,
        )
        .map_err(|e| CoreError::deserialization(format!("RoleManagement node list: {e}")))?;
        Ok(NodeList::from_stack_value(value)?.into_nodes())
    }

    /// Serializes an empty node list (C# returns an empty `ECPoint[]`, not `null`,
    /// when no designation exists).
    fn empty_node_list() -> CoreResult<Vec<u8>> {
        let item = NodeList::new(Vec::new()).to_stack_value();
        BinarySerializer::serialize_stack_value_default(&item)
            .map_err(|e| CoreError::invalid_operation(format!("RoleManagement empty list: {e}")))
    }

    /// The designation storage key `(RoleManagement.ID, [role_byte, index_be])`.
    fn designation_key(role_byte: u8, index: u32) -> StorageKey {
        StorageKey::new(
            RoleManagement::ID,
            crate::keys::prefixed_with_u32_be(role_byte, index),
        )
    }

    /// Builds the persisted `StackValue::Array` representation for C# `NodeList`.
    fn nodes_to_stack_value(points: &[ECPoint]) -> StackValue {
        NodeList::new(points.to_vec()).to_stack_value()
    }

    /// Adapts the canonical node-list `StackValue` projection to the live VM
    /// notification boundary, preserving the caller-provided order.
    fn nodes_to_event_array(points: &[ECPoint]) -> CoreResult<StackItem> {
        StackItem::try_from(Self::nodes_to_stack_value(points)).map_err(|error| {
            CoreError::invalid_operation(format!("RoleManagement event node list: {error}"))
        })
    }

    /// Serializes a node list as C# `NodeList` stores it: a `BinarySerializer` array
    /// of compressed EC-point byte strings, with the points sorted ascending
    /// (`list.Sort()`). The stored order differs from the event's `newNodes` order
    /// (which preserves the caller's input order).
    fn encode_node_list(points: &[ECPoint]) -> CoreResult<Vec<u8>> {
        let mut sorted = points.to_vec();
        sorted.sort();
        BinarySerializer::serialize_stack_value_default(&Self::nodes_to_stack_value(&sorted))
            .map_err(|e| CoreError::invalid_operation(format!("RoleManagement node list: {e}")))
    }

    /// Decodes + validates the `nodes` Array argument: 1..=32 compressed EC points
    /// (C# `nodes.Length == 0 || nodes.Length > 32` guard).
    fn parse_nodes_arg(bytes: &[u8]) -> CoreResult<Vec<ECPoint>> {
        let points = Self::decode_node_list(bytes)?;
        if points.is_empty() || points.len() > 32 {
            return Err(CoreError::invalid_operation(format!(
                "RoleManagement: nodes count {} must be between 1 and 32",
                points.len()
            )));
        }
        Ok(points)
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
            state.push(Self::nodes_to_event_array(old_nodes)?);
            state.push(Self::nodes_to_event_array(new_nodes)?);
        }
        Ok(state)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NodeList {
    nodes: Vec<ECPoint>,
}

impl NodeList {
    fn new(nodes: Vec<ECPoint>) -> Self {
        Self { nodes }
    }

    fn into_nodes(self) -> Vec<ECPoint> {
        self.nodes
    }

    fn to_stack_value(&self) -> StackValue {
        StackValue::Array(
            0,
            self.nodes
                .iter()
                .map(|point| StackValue::ByteString(point.to_bytes()))
                .collect(),
        )
    }

    fn from_stack_value(stack_value: StackValue) -> CoreResult<Self> {
        let StackValue::Array(0, items) = stack_value else {
            return Err(CoreError::invalid_data(
                "RoleManagement node list is not an array",
            ));
        };
        let mut nodes = Vec::with_capacity(items.len());
        for entry in items {
            let bytes = entry.to_byte_string_bytes().ok_or_else(|| {
                CoreError::invalid_data("RoleManagement node entry is not byte-like")
            })?;
            nodes.push(ECPoint::from_bytes(&bytes).map_err(|e| {
                CoreError::invalid_data(format!("RoleManagement node EC point: {e}"))
            })?);
        }
        Ok(Self { nodes })
    }
}

neo_vm::impl_interoperable_via_stack_value!(NodeList);

static ROLE_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    vec![
        NativeMethod::new(
            "getDesignatedByRole".to_string(),
            1 << 15,
            true,
            CallFlags::READ_STATES.bits(),
            vec![
                ContractParameterType::Integer,
                ContractParameterType::Integer,
            ],
            ContractParameterType::Array,
        )
        .with_parameter_names(["role", "index"]),
        // Committee-gated writer that emits a Designation event (States|AllowNotify).
        NativeMethod::new(
            "designateAsRole".to_string(),
            1 << 15,
            false,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
            vec![ContractParameterType::Integer, ContractParameterType::Array],
            ContractParameterType::Void,
        )
        .with_parameter_names(["role", "nodes"]),
    ]
});

/// The dual `Designation` event registration (RoleManagement.cs:27-37): both
/// share order 0 and exactly one is active at any height. V0
/// `(Role, BlockIndex)` is genesis-active and DeprecatedIn `HF_Echidna`
/// (the trailing ctor argument); V1 adds the `Old`/`New` node arrays and is
/// ActiveIn `HF_Echidna`.
static ROLE_EVENTS: LazyLock<Vec<NativeEvent>> = LazyLock::new(|| {
    vec![
        NativeEvent::new(
            0,
            "Designation",
            &[
                ("Role", ContractParameterType::Integer),
                ("BlockIndex", ContractParameterType::Integer),
            ],
        )
        .with_deprecated_in(Hardfork::HfEchidna),
        NativeEvent::new(
            0,
            "Designation",
            &[
                ("Role", ContractParameterType::Integer),
                ("BlockIndex", ContractParameterType::Integer),
                ("Old", ContractParameterType::Array),
                ("New", ContractParameterType::Array),
            ],
        )
        .with_active_in(Hardfork::HfEchidna),
    ]
});

impl NativeContract for RoleManagement {
    fn id(&self) -> i32 {
        Self::ID
    }

    fn hash(&self) -> UInt160 {
        Self::script_hash()
    }

    fn name(&self) -> &str {
        Self::NAME
    }

    fn methods(&self) -> &[NativeMethod] {
        &ROLE_METHODS
    }

    fn event_descriptors(&self) -> &[NativeEvent] {
        &ROLE_EVENTS
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        match method {
            "getDesignatedByRole" => {
                let role_value = args
                    .first()
                    .map(|b| BigInt::from_signed_bytes_le(b))
                    .and_then(|b| b.to_u32())
                    .ok_or_else(|| {
                        CoreError::invalid_operation("RoleManagement: missing/invalid role")
                    })?;
                // C# validates the role against the Role enum.
                let role_byte = match role_value {
                    4 | 8 | 16 | 32 => role_value as u8,
                    _ => {
                        return Err(CoreError::invalid_operation(format!(
                            "RoleManagement: role {role_value} is not valid"
                        )));
                    }
                };
                let index = args
                    .get(1)
                    .map(|b| BigInt::from_signed_bytes_le(b))
                    .and_then(|b| b.to_u32())
                    .ok_or_else(|| {
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

                match self.find_designation_value(&snapshot, role_byte, index) {
                    // The stored value is already the BinarySerializer-encoded
                    // node-list array — exactly the Array return wants.
                    Some(value) => Ok(value),
                    None => Self::empty_node_list(),
                }
            }
            "designateAsRole" => {
                // C# order: validate nodes (1..32) -> validate role ->
                // AssertCommittee -> require persisting block -> reject duplicate
                // -> store sorted -> emit Designation event.
                let role_value = args
                    .first()
                    .map(|b| BigInt::from_signed_bytes_le(b))
                    .and_then(|b| b.to_u32())
                    .ok_or_else(|| {
                        CoreError::invalid_operation("RoleManagement: missing/invalid role")
                    })?;
                let nodes_bytes = args.get(1).ok_or_else(|| {
                    CoreError::invalid_operation("RoleManagement: missing nodes argument")
                })?;
                let nodes = Self::parse_nodes_arg(nodes_bytes)?;
                let role_byte = match role_value {
                    4 | 8 | 16 | 32 => role_value as u8,
                    _ => {
                        return Err(CoreError::invalid_operation(format!(
                            "RoleManagement: role {role_value} is not valid"
                        )));
                    }
                };

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
                    StorageItem::from_bytes(Self::encode_node_list(&nodes)?),
                );

                // Emit the Designation event; from HF_Echidna it also carries the
                // previously-effective (at block_index) and new node lists.
                let echidna = engine.is_hardfork_enabled(Hardfork::HfEchidna);
                let old_nodes = if echidna {
                    match self.find_designation_value(&snapshot, role_byte, block_index) {
                        Some(value) => Self::decode_node_list(&value)?,
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
                    .send_notification(Self::script_hash(), "Designation".to_string(), state)
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
    use super::*;
    use neo_storage::StorageItem;

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
    fn encode_node_list_sorts_and_round_trips() {
        // Two distinct valid points, given in non-sorted order.
        let a = sample_point();
        let b = ECPoint::from_bytes(&hex_to_bytes(
            "02df48f60e8f3e01c48ff40b9b7f1310d7a8b2a193188befe1c2e3df740e895093",
        ))
        .unwrap();
        let input = vec![a.clone(), b.clone()];
        // encode_node_list stores them sorted; decode_node_list reads them back.
        let encoded = RoleManagement::encode_node_list(&input).unwrap();
        let mut expected = input.clone();
        expected.sort();
        let expected_value = StackValue::Array(
            expected
                .iter()
                .map(|point| StackValue::ByteString(point.to_bytes()))
                .collect(),
        );
        let expected_encoded = BinarySerializer::serialize_stack_value_default(&expected_value)
            .expect("expected node-list StackValue serializes");
        assert_eq!(encoded, expected_encoded);
        let decoded = RoleManagement::decode_node_list(&encoded).unwrap();
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

        let source = include_str!("role_management.rs");
        let decoder = slice_between(source, "fn decode_node_list", "fn empty_node_list");
        assert!(decoder.contains("deserialize_stack_value_with_limits"));
        assert!(decoder.contains("NodeList::from_stack_value"));
        assert!(!decoder.contains("StackValue::Array"));

        let empty_encoder = slice_between(source, "fn empty_node_list", "fn designation_key");
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
        assert!(RoleManagement::parse_nodes_arg(&empty).is_err());
        // One valid node -> accepted.
        let one = RoleManagement::encode_node_list(&[sample_point()]).unwrap();
        assert_eq!(RoleManagement::parse_nodes_arg(&one).unwrap().len(), 1);
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
