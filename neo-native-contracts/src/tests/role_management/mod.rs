//! # neo-native-contracts::tests::role_management
//!
//! Test module grouping Native RoleManagement state and designated-node
//! behavior. coverage for neo-native-contracts.
//!
//! ## Boundary
//!
//! This is test/benchmark-only code for neo-native-contracts; it may assemble
//! fixtures but must not introduce production behavior.
//!
//! ## Contents
//!
//! - Test modules and fixtures: grouped coverage for the surrounding domain.

use super::node_list::{self, NodeList};
use super::storage;
use super::*;
use crate::Role;
use neo_crypto::ECPoint;
use neo_primitives::{CallFlags, ContractParameterType};
use neo_serialization::BinarySerializer;
use neo_storage::StorageItem;
use neo_storage::persistence::DataCache;
use neo_vm::{Interoperable, StackItem};
use neo_vm_rs::StackValue;

/// Structural equality for StackValue that ignores the reference-identity ids
/// on compound variants. Collection identity is not part of serialized
/// stack data, so structural equality is the correct notion for round-trip / shape
/// assertions.
fn stack_value_struct_eq(a: &neo_vm_rs::StackValue, b: &neo_vm_rs::StackValue) -> bool {
    use neo_vm_rs::StackValue::*;
    match (a, b) {
        (Buffer(x), Buffer(y)) => x == y,
        (Array(x), Array(y)) | (Struct(x), Struct(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| stack_value_struct_eq(p, q))
        }
        (Map(x), Map(y)) => {
            x.len() == y.len()
                && x.iter().zip(y).all(|((k1, v1), (k2, v2))| {
                    stack_value_struct_eq(k1, k2) && stack_value_struct_eq(v1, v2)
                })
        }
        _ => a == b,
    }
}

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
    let source = include_str!("../../role_management/invoke.rs");
    let start = source
        .find("fn invoke_native(")
        .expect("RoleManagement invoke_native exists");
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
    let expected_value = StackValue::Array(vec![
        StackValue::ByteString(a.to_bytes()),
        StackValue::ByteString(b.to_bytes()),
    ]);

    let trait_value = Interoperable::to_stack_value(&state).unwrap();
    assert!(
        stack_value_struct_eq(&trait_value, &expected_value),
        "structural StackValue mismatch: {trait_value:?} vs {expected_value:?}"
    );

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

    let source = include_str!("../../role_management/node_list.rs");
    let decoder = slice_between(source, "fn decode_node_list", "fn empty_node_list");
    assert!(decoder.contains("decode_stack_value"));
    assert!(decoder.contains("NodeList::from_stack_value"));
    assert!(!decoder.contains("StackValue::Array"));

    let empty_encoder = slice_between(
        source,
        "fn empty_node_list",
        "/// Builds the persisted `StackValue::Array`",
    );
    assert!(empty_encoder.contains("NodeList::new"));
    assert!(empty_encoder.contains("encode_storage_struct"));
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
        BinarySerializer::serialize_stack_value_default(&StackValue::Array(Vec::new())).unwrap();
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
    let list = BinarySerializer::serialize_stack_value_default(&StackValue::Array(vec![
        StackValue::ByteString(point.to_bytes()),
    ]))
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
