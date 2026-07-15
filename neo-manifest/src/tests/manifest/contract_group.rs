use super::*;
use neo_primitives::hex_util;

fn stack_item_struct_eq(a: &neo_vm::StackItem, b: &neo_vm::StackItem) -> bool {
    a.equals(b).unwrap_or(false)
}

fn sample_group() -> ContractGroup {
    let encoded =
        hex_util::decode_hex("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c")
            .expect("hex");
    let pub_key = ECPoint::decode(&encoded, ECCurve::secp256r1()).expect("valid ECPoint");

    ContractGroup::new(pub_key, vec![0xAB; 64])
}

#[test]
fn contract_group_projects_to_neo_vm_stack_item() {
    let group = sample_group();
    let pub_key_bytes = group.pub_key.encode_point(true).expect("compressed key");

    let left = group.to_stack_item();
    let right = StackItem::from_struct(vec![
        StackItem::ByteString(pub_key_bytes),
        StackItem::ByteString(vec![0xAB; 64]),
    ]);
    assert!(
        stack_item_struct_eq(&left, &right),
        "structural StackItem mismatch: {left:?} vs {right:?}"
    );
}

#[test]
fn contract_group_reads_from_neo_vm_stack_item() {
    let group = sample_group();
    let pub_key_bytes = group.pub_key.encode_point(true).expect("compressed key");

    let decoded = ContractGroup::try_from_stack_item(&StackItem::from_struct(vec![
        StackItem::ByteString(pub_key_bytes),
        StackItem::ByteString(vec![0xCD; 64]),
    ]))
    .unwrap();

    assert_eq!(decoded.pub_key, group.pub_key);
    assert_eq!(decoded.signature, vec![0xCD; 64]);
}
