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
use neo_execution::{ApplicationEngine, NativeContract, NativeMethod};
use neo_primitives::{CallFlags, ContractParameterType, UInt160};
use neo_serialization::BinarySerializer;
use neo_storage::persistence::{DataCache, SeekDirection};
use neo_storage::{StorageItem, StorageKey};
use neo_vm::StackItem;
use neo_vm_rs::ExecutionEngineLimits;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

use crate::hashes::ROLE_MANAGEMENT_HASH;
use crate::role::Role;
use crate::LedgerContract;

/// Lazily-initialised script-hash handle for the RoleManagement contract.
pub static ROLE_MANAGEMENT_HASH_REF: LazyLock<UInt160> =
    LazyLock::new(|| *ROLE_MANAGEMENT_HASH);

/// The RoleManagement native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct RoleManagement;

impl RoleManagement {
    /// Stable native contract id (matches C# `RoleManagement`).
    pub const ID: i32 = -8;

    /// Construct a new `RoleManagement` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the RoleManagement script hash.
    pub fn script_hash() -> UInt160 {
        *ROLE_MANAGEMENT_HASH_REF
    }

    /// Looks up the public keys designated for `role`, effective at block
    /// `height` (the most recent designation with index ≤ `height`).
    pub fn get_designated_by_role_at(
        &self,
        snapshot: &DataCache,
        role: Role,
        height: u32,
    ) -> CoreResult<Vec<ECPoint>> {
        match find_designation_value(snapshot, role.as_byte(), height) {
            Some(value) => decode_node_list(&value),
            None => Ok(Vec::new()),
        }
    }
}

/// Finds the serialized node-list value for `role_byte` effective at `index`:
/// the entry with the greatest designation index that is ≤ `index`.
///
/// Designations are stored under key `(RoleManagement.ID, [role_byte, index_be])`.
/// A `Backward` prefix scan yields them in descending designation-index order, so
/// the first one with `designation_index <= index` is the effective designation —
/// equivalent to C#'s `FindRange((role, index), (role), Backward).FirstOrDefault`.
fn find_designation_value(snapshot: &DataCache, role_byte: u8, index: u32) -> Option<Vec<u8>> {
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
    let item = BinarySerializer::deserialize(value, &ExecutionEngineLimits::default(), None)
        .map_err(|e| CoreError::deserialization(format!("RoleManagement node list: {e}")))?;
    let StackItem::Array(array) = item else {
        return Err(CoreError::invalid_data(
            "RoleManagement node list is not an array",
        ));
    };
    let mut points = Vec::new();
    for entry in array.items() {
        let bytes = entry
            .as_bytes()
            .map_err(|e| CoreError::invalid_data(format!("RoleManagement node bytes: {e}")))?;
        points.push(
            ECPoint::from_bytes(&bytes)
                .map_err(|e| CoreError::invalid_data(format!("RoleManagement node EC point: {e}")))?,
        );
    }
    Ok(points)
}

/// Serializes an empty node list (C# returns an empty `ECPoint[]`, not `null`,
/// when no designation exists).
fn empty_node_list() -> CoreResult<Vec<u8>> {
    BinarySerializer::serialize(&StackItem::from_array(Vec::new()), &ExecutionEngineLimits::default())
        .map_err(|e| CoreError::invalid_operation(format!("RoleManagement empty list: {e}")))
}

/// The designation storage key `(RoleManagement.ID, [role_byte, index_be])`.
fn designation_key(role_byte: u8, index: u32) -> StorageKey {
    let mut key = Vec::with_capacity(5);
    key.push(role_byte);
    key.extend_from_slice(&index.to_be_bytes());
    StorageKey::new(RoleManagement::ID, key)
}

/// Builds a `StackItem::Array` of compressed EC-point byte strings, preserving
/// the given order (used for the event's node arrays).
fn nodes_to_array(points: &[ECPoint]) -> StackItem {
    StackItem::from_array(
        points
            .iter()
            .map(|p| StackItem::from_byte_string(p.to_bytes()))
            .collect::<Vec<_>>(),
    )
}

/// Serializes a node list as C# `NodeList` stores it: a `BinarySerializer` array
/// of compressed EC-point byte strings, with the points sorted ascending
/// (`list.Sort()`). The stored order differs from the event's `newNodes` order
/// (which preserves the caller's input order).
fn encode_node_list(points: &[ECPoint]) -> CoreResult<Vec<u8>> {
    let mut sorted = points.to_vec();
    sorted.sort();
    BinarySerializer::serialize(&nodes_to_array(&sorted), &ExecutionEngineLimits::default())
        .map_err(|e| CoreError::invalid_operation(format!("RoleManagement node list: {e}")))
}

/// Decodes + validates the `nodes` Array argument: 1..=32 compressed EC points
/// (C# `nodes.Length == 0 || nodes.Length > 32` guard).
fn parse_nodes_arg(bytes: &[u8]) -> CoreResult<Vec<ECPoint>> {
    let points = decode_node_list(bytes)?;
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
) -> Vec<StackItem> {
    let mut state = vec![
        StackItem::from_int(i64::from(role_byte)),
        StackItem::from_int(i64::from(block_index)),
    ];
    if echidna {
        state.push(nodes_to_array(old_nodes));
        state.push(nodes_to_array(new_nodes));
    }
    state
}

static ROLE_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    vec![
        NativeMethod::new(
            "getDesignatedByRole".to_string(),
            1 << 15,
            true,
            CallFlags::READ_STATES.bits(),
            vec![ContractParameterType::Integer, ContractParameterType::Integer],
            ContractParameterType::Array,
        ),
        // Committee-gated writer that emits a Designation event (States|AllowNotify).
        NativeMethod::new(
            "designateAsRole".to_string(),
            1 << 15,
            false,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
            vec![ContractParameterType::Integer, ContractParameterType::Array],
            ContractParameterType::Void,
        ),
    ]
});

impl NativeContract for RoleManagement {
    fn id(&self) -> i32 {
        Self::ID
    }

    fn hash(&self) -> UInt160 {
        *ROLE_MANAGEMENT_HASH_REF
    }

    fn name(&self) -> &str {
        "RoleManagement"
    }

    fn methods(&self) -> &[NativeMethod] {
        &ROLE_METHODS
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
                        )))
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

                match find_designation_value(&snapshot, role_byte, index) {
                    // The stored value is already the BinarySerializer-encoded
                    // node-list array — exactly the Array return wants.
                    Some(value) => Ok(value),
                    None => empty_node_list(),
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
                let nodes = parse_nodes_arg(nodes_bytes)?;
                let role_byte = match role_value {
                    4 | 8 | 16 | 32 => role_value as u8,
                    _ => {
                        return Err(CoreError::invalid_operation(format!(
                            "RoleManagement: role {role_value} is not valid"
                        )))
                    }
                };

                // C# AssertCommittee.
                if !engine.check_committee_witness().map_err(|e| {
                    CoreError::invalid_operation(format!("designateAsRole committee check: {e}"))
                })? {
                    return Err(CoreError::invalid_operation(
                        "designateAsRole requires committee authorization",
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
                let key = designation_key(role_byte, index);
                if snapshot.get(&key).is_some() {
                    return Err(CoreError::invalid_operation(
                        "designateAsRole: role already designated at this index",
                    ));
                }
                snapshot.add(key, StorageItem::from_bytes(encode_node_list(&nodes)?));

                // Emit the Designation event; from HF_Echidna it also carries the
                // previously-effective (at block_index) and new node lists.
                let echidna = engine.is_hardfork_enabled(Hardfork::HfEchidna);
                let old_nodes = if echidna {
                    match find_designation_value(&snapshot, role_byte, block_index) {
                        Some(value) => decode_node_list(&value)?,
                        None => Vec::new(),
                    }
                } else {
                    Vec::new()
                };
                let state =
                    designation_event_state(role_byte, block_index, echidna, &old_nodes, &nodes);
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
        ECPoint::from_bytes(
            &hex_to_bytes("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c"),
        )
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
        assert_eq!(NativeContract::id(&c), -8);
        assert_eq!(NativeContract::name(&c), "RoleManagement");
        assert_eq!(NativeContract::hash(&c), *ROLE_MANAGEMENT_HASH);
        let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["getDesignatedByRole", "designateAsRole"]);
        // The writer is non-safe with write + notify flags and a Void return.
        let d = c.methods().iter().find(|m| m.name == "designateAsRole").unwrap();
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
        let decoded = decode_node_list(&encode_node_list(&input).unwrap()).unwrap();
        let mut expected = input.clone();
        expected.sort();
        assert_eq!(decoded, expected);
    }

    #[test]
    fn parse_nodes_arg_enforces_1_to_32() {
        // Empty array -> rejected.
        let empty = BinarySerializer::serialize(
            &StackItem::from_array(Vec::<StackItem>::new()),
            &ExecutionEngineLimits::default(),
        )
        .unwrap();
        assert!(parse_nodes_arg(&empty).is_err());
        // One valid node -> accepted.
        let one = encode_node_list(&[sample_point()]).unwrap();
        assert_eq!(parse_nodes_arg(&one).unwrap().len(), 1);
    }

    #[test]
    fn designation_event_state_shape_matches_hardfork() {
        let new_nodes = vec![sample_point()];
        let old_nodes: Vec<ECPoint> = Vec::new();

        // Pre-Echidna: [role, blockIndex].
        let pre = designation_event_state(8, 41, false, &old_nodes, &new_nodes);
        assert_eq!(pre.len(), 2);
        assert_eq!(pre[0].as_int().unwrap(), num_bigint::BigInt::from(8));
        assert_eq!(pre[1].as_int().unwrap(), num_bigint::BigInt::from(41));

        // Echidna: [role, blockIndex, oldNodes(Array), newNodes(Array)].
        let post = designation_event_state(8, 41, true, &old_nodes, &new_nodes);
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
        assert!(RoleManagement::new()
            .get_designated_by_role_at(&cache, Role::Oracle, 100)
            .unwrap()
            .is_empty());

        // Designate the Oracle role at index 10 (a 1-element node list).
        let list = BinarySerializer::serialize(
            &StackItem::from_array(vec![StackItem::from_byte_string(point.to_bytes())]),
            &ExecutionEngineLimits::default(),
        )
        .unwrap();
        cache.add(
            designation_key(Role::Oracle.as_byte(), 10),
            StorageItem::from_bytes(list),
        );

        // Before the designation -> still empty; at/after -> the designated key.
        assert!(RoleManagement::new()
            .get_designated_by_role_at(&cache, Role::Oracle, 9)
            .unwrap()
            .is_empty());
        let got = RoleManagement::new()
            .get_designated_by_role_at(&cache, Role::Oracle, 25)
            .unwrap();
        assert_eq!(got, vec![point.clone()]);

        // A different role is unaffected.
        assert!(RoleManagement::new()
            .get_designated_by_role_at(&cache, Role::StateValidator, 25)
            .unwrap()
            .is_empty());
    }
}
