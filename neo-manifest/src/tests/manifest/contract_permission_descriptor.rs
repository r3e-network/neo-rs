use super::*;
use neo_crypto::{ECPoint, Secp256k1Crypto, Secp256r1Crypto};
use neo_primitives::hex_util;
use neo_vm_rs::StackValue;

fn group_key() -> ECPoint {
    let private_key = [1u8; 32];
    let public_key = Secp256r1Crypto::derive_public_key(&private_key).expect("public key");
    ECPoint::from_bytes(&public_key).expect("group public key")
}

#[test]
fn permission_descriptor_projects_to_neo_vm_rs_stack_value() {
    let hash = UInt160::from_bytes(&[0x11; 20]).expect("hash");
    let group = group_key();
    let group_bytes = group.encode_point(true).expect("compressed key");

    assert_eq!(
        ContractPermissionDescriptor::create_wildcard().to_stack_value(),
        StackValue::Null
    );
    assert_eq!(
        ContractPermissionDescriptor::create_hash(hash).to_stack_value(),
        StackValue::ByteString(hash.to_bytes())
    );
    assert_eq!(
        ContractPermissionDescriptor::create_group(group).to_stack_value(),
        StackValue::ByteString(group_bytes)
    );
}

#[test]
fn permission_descriptor_stack_item_projection_matches_stack_value_projection() {
    let descriptor =
        ContractPermissionDescriptor::create_hash(UInt160::from_bytes(&[0x22; 20]).unwrap());
    let expected = StackItem::try_from(descriptor.to_stack_value()).unwrap();

    assert_eq!(descriptor.to_stack_item(), expected);
}

#[test]
fn permission_descriptor_reads_from_neo_vm_rs_stack_value() {
    let hash = UInt160::from_bytes(&[0x33; 20]).expect("hash");
    let group = group_key();
    let group_bytes = group.encode_point(true).expect("compressed key");

    assert_eq!(
        ContractPermissionDescriptor::from_stack_value(StackValue::Null).unwrap(),
        ContractPermissionDescriptor::Wildcard
    );
    assert_eq!(
        ContractPermissionDescriptor::from_stack_value(StackValue::ByteString(hash.to_bytes()))
            .unwrap(),
        ContractPermissionDescriptor::Hash(hash)
    );
    assert_eq!(
        ContractPermissionDescriptor::from_stack_value(StackValue::Buffer(
            neo_vm_rs::next_stack_item_id(),
            group_bytes
        ))
        .unwrap(),
        ContractPermissionDescriptor::Group(group)
    );
}

#[test]
fn permission_descriptor_rejects_invalid_stack_byte_lengths_like_csharp() {
    assert!(
        ContractPermissionDescriptor::from_stack_value(StackValue::ByteString(Vec::new())).is_err()
    );
    assert!(
        ContractPermissionDescriptor::from_stack_value(StackValue::ByteString(b"*".to_vec()))
            .is_err()
    );
}

#[test]
fn permission_descriptor_from_json_uses_csharp_lengths_and_curve() {
    let hash = UInt160::from_bytes(&[0x44; 20]).expect("hash");
    assert_eq!(
        ContractPermissionDescriptor::from_json(&serde_json::Value::String(hash.to_string()))
            .unwrap(),
        ContractPermissionDescriptor::Hash(hash)
    );
    assert!(
        ContractPermissionDescriptor::from_json(&serde_json::Value::String(
            hash.to_string().trim_start_matches("0x").to_string()
        ))
        .is_err()
    );

    let group = group_key();
    let compressed = group.encode_point(true).expect("compressed group");
    assert_eq!(
        ContractPermissionDescriptor::from_json(&serde_json::Value::String(hex_util::encode_hex(
            &compressed
        )))
        .unwrap(),
        ContractPermissionDescriptor::Group(group.clone())
    );

    let uncompressed = group.encode_point(false).expect("uncompressed group");
    assert!(
        ContractPermissionDescriptor::from_json(&serde_json::Value::String(hex_util::encode_hex(
            &uncompressed
        )))
        .is_err()
    );
}

#[test]
fn permission_descriptor_rejects_non_secp256r1_stack_group_like_csharp() {
    let private_key = [2u8; 32];
    let k1_group = Secp256k1Crypto::derive_public_key(&private_key).expect("secp256k1 public key");

    assert!(
        ContractPermissionDescriptor::from_stack_value(StackValue::ByteString(k1_group.to_vec()))
            .is_err()
    );
}
