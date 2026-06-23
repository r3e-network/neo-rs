use super::*;

fn sample_group() -> ContractGroup {
    let encoded = hex::decode("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c")
        .expect("hex");
    let pub_key = ECPoint::decode(&encoded, ECCurve::secp256r1()).expect("valid ECPoint");

    ContractGroup::new(pub_key, vec![0xAB; 64])
}

#[test]
fn contract_group_projects_to_neo_vm_rs_stack_value() {
    let group = sample_group();
    let pub_key_bytes = group.pub_key.encode_point(true).expect("compressed key");

    assert_eq!(
        group.to_stack_value(),
        StackValue::Struct(
            0,
            vec![
                StackValue::ByteString(pub_key_bytes),
                StackValue::ByteString(vec![0xAB; 64]),
            ]
        )
    );
}

#[test]
fn contract_group_reads_from_neo_vm_rs_stack_value() {
    let group = sample_group();
    let pub_key_bytes = group.pub_key.encode_point(true).expect("compressed key");

    let decoded = ContractGroup::try_from_stack_value(StackValue::Struct(
        0,
        vec![
            StackValue::ByteString(pub_key_bytes),
            StackValue::ByteString(vec![0xCD; 64]),
        ],
    ))
    .unwrap();

    assert_eq!(decoded.pub_key, group.pub_key);
    assert_eq!(decoded.signature, vec![0xCD; 64]);
}
