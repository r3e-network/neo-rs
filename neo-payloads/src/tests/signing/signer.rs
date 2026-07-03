use super::*;
use neo_vm::Interoperable;
use neo_vm_rs::StackValue;

/// Structural equality for StackValue that ignores the reference-identity ids
/// on compound variants (neo-vm-rs 0.2.0 compares compounds by id; tests want
/// value equality). The id is not serialized, so structural equality is the
/// correct notion for round-trip / shape assertions.
fn stack_value_struct_eq(a: &neo_vm_rs::StackValue, b: &neo_vm_rs::StackValue) -> bool {
    use neo_vm_rs::StackValue::*;
    match (a, b) {
        (Buffer(_, x), Buffer(_, y)) => x == y,
        (Array(_, x), Array(_, y)) | (Struct(_, x), Struct(_, y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| stack_value_struct_eq(p, q))
        }
        (Map(_, x), Map(_, y)) => {
            x.len() == y.len()
                && x.iter().zip(y).all(|((k1, v1), (k2, v2))| {
                    stack_value_struct_eq(k1, k2) && stack_value_struct_eq(v1, v2)
                })
        }
        _ => a == b,
    }
}

#[test]
fn signer_projects_to_neo_vm_rs_stack_value() {
    let account = UInt160::from_bytes(&[0x11; UINT160_SIZE]).unwrap();
    let allowed_contract = UInt160::from_bytes(&[0x22; UINT160_SIZE]).unwrap();
    let rule = WitnessRule::new(
        WitnessRuleAction::Allow,
        WitnessCondition::Boolean { value: true },
    );
    let scopes = WitnessScope::CUSTOM_CONTRACTS | WitnessScope::WITNESS_RULES;
    let mut signer = Signer::new(account, scopes);
    signer.allowed_contracts.push(allowed_contract);
    signer.rules.push(rule.clone());

    let left = signer.to_stack_value();
    let right = StackValue::Array(
        neo_vm_rs::next_stack_item_id(),
        vec![
            StackValue::ByteString(account.to_bytes()),
            StackValue::Integer(i64::from(scopes.bits())),
            StackValue::Array(
                neo_vm_rs::next_stack_item_id(),
                vec![StackValue::ByteString(allowed_contract.to_bytes())],
            ),
            StackValue::Array(neo_vm_rs::next_stack_item_id(), Vec::new()),
            StackValue::Array(neo_vm_rs::next_stack_item_id(), vec![rule.to_stack_value()]),
        ],
    );
    assert!(
        stack_value_struct_eq(&left, &right),
        "structural StackValue mismatch: {left:?} vs {right:?}"
    );
}

#[test]
fn signer_interoperable_to_stack_value_matches_inherent() {
    let account = UInt160::from_bytes(&[0x33; UINT160_SIZE]).unwrap();
    let signer = Signer::new(account, WitnessScope::CALLED_BY_ENTRY);

    let expected = signer.to_stack_value();
    let interop = Interoperable::to_stack_value(&signer).unwrap();
    assert!(
        stack_value_struct_eq(&interop, &expected),
        "structural StackValue mismatch: {interop:?} vs {expected:?}"
    );
}

#[test]
fn signer_to_json_keeps_empty_scope_arrays_like_csharp() {
    let account = UInt160::from_bytes(&[0x44; UINT160_SIZE]).unwrap();
    let signer = Signer::new(
        account,
        WitnessScope::CUSTOM_CONTRACTS | WitnessScope::CUSTOM_GROUPS | WitnessScope::WITNESS_RULES,
    );

    let json = signer.to_json();

    assert_eq!(
        json.get("scopes"),
        Some(&serde_json::json!(
            "CustomContracts, CustomGroups, WitnessRules"
        )),
        "C# Signer.ToJson writes WitnessScope through enum ToString()"
    );
    assert_eq!(
        json.get("allowedcontracts"),
        Some(&serde_json::json!([])),
        "C# Signer.ToJson emits allowedcontracts whenever CustomContracts is set"
    );
    assert_eq!(
        json.get("allowedgroups"),
        Some(&serde_json::json!([])),
        "C# Signer.ToJson emits allowedgroups whenever CustomGroups is set"
    );
    assert_eq!(
        json.get("rules"),
        Some(&serde_json::json!([])),
        "C# Signer.ToJson emits rules whenever WitnessRules is set"
    );
}

#[test]
fn signer_from_json_scope_is_case_sensitive_and_comma_separated_like_csharp_v3100() {
    let account = UInt160::from_bytes(&[0x55; UINT160_SIZE]).unwrap();
    let valid = serde_json::json!({
        "account": account.to_string(),
        "scopes": "CalledByEntry, CustomContracts",
        "allowedcontracts": [],
    });
    assert_eq!(
        Signer::from_json(&valid).unwrap().scopes,
        WitnessScope::CALLED_BY_ENTRY | WitnessScope::CUSTOM_CONTRACTS
    );

    let lower_case = serde_json::json!({
        "account": account.to_string(),
        "scopes": "global",
    });
    assert!(Signer::from_json(&lower_case).is_err());

    let pipe_separated = serde_json::json!({
        "account": account.to_string(),
        "scopes": "CalledByEntry | CustomContracts",
    });
    assert!(Signer::from_json(&pipe_separated).is_err());
}

#[test]
fn signer_from_json_accepts_uppercase_hex_prefix_for_allowed_groups() {
    let account = UInt160::from_bytes(&[0x56; UINT160_SIZE]).unwrap();
    let group =
        hex_decode("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c").unwrap();
    let json = serde_json::json!({
        "account": account.to_string(),
        "scopes": "CustomGroups",
        "allowedgroups": [format!("0X{}", hex_encode(&group))],
    });

    let signer = Signer::from_json(&json).unwrap();

    assert_eq!(signer.allowed_groups.len(), 1);
    assert_eq!(signer.allowed_groups[0].to_bytes(), group);
}

#[test]
fn signer_deserialize_rejects_global_combined_scope() {
    let mut data = vec![0u8; UINT160_SIZE];
    data.push((WitnessScope::GLOBAL | WitnessScope::CALLED_BY_ENTRY).bits());
    let mut reader = MemoryReader::new(&data);

    assert!(<Signer as Serializable>::deserialize(&mut reader).is_err());
}

#[test]
fn signer_deserialize_rejects_too_many_allowed_contracts() {
    let mut writer = BinaryWriter::new();
    writer.write_serializable(&UInt160::zero()).unwrap();
    writer
        .write_u8(WitnessScope::CUSTOM_CONTRACTS.bits())
        .unwrap();
    writer.write_var_uint((MAX_SUBITEMS + 1) as u64).unwrap();
    for value in 0..=MAX_SUBITEMS {
        let contract = UInt160::from_bytes(&[value as u8; UINT160_SIZE]).unwrap();
        writer.write_serializable(&contract).unwrap();
    }
    let data = writer.into_bytes();
    let mut reader = MemoryReader::new(&data);

    assert!(<Signer as Serializable>::deserialize(&mut reader).is_err());
}

#[test]
fn signer_deserialize_rejects_too_many_rules() {
    let mut writer = BinaryWriter::new();
    writer.write_serializable(&UInt160::zero()).unwrap();
    writer.write_u8(WitnessScope::WITNESS_RULES.bits()).unwrap();
    writer.write_var_uint((MAX_SUBITEMS + 1) as u64).unwrap();
    let rule = WitnessRule::new(
        WitnessRuleAction::Allow,
        WitnessCondition::Boolean { value: true },
    );
    for _ in 0..=MAX_SUBITEMS {
        writer.write_serializable(&rule).unwrap();
    }
    let data = writer.into_bytes();
    let mut reader = MemoryReader::new(&data);

    assert!(<Signer as Serializable>::deserialize(&mut reader).is_err());
}

#[test]
fn signer_deserialize_accepts_uncompressed_allowed_group_like_csharp_v3100() {
    let compressed =
        hex_decode("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c").unwrap();
    let point = ECPoint::from_bytes_with_curve(ECCurve::secp256r1(), &compressed).unwrap();
    let uncompressed = point.encode_point(false).unwrap();
    assert_eq!(uncompressed.len(), 65);

    let mut wire = Vec::new();
    wire.extend_from_slice(&UInt160::zero().to_bytes());
    wire.push(WitnessScope::CUSTOM_GROUPS.bits());
    wire.push(1);
    wire.extend_from_slice(&uncompressed);

    let mut reader = MemoryReader::new(&wire);
    let signer = <Signer as Serializable>::deserialize(&mut reader).unwrap();

    assert_eq!(signer.allowed_groups.len(), 1);
    assert_eq!(signer.allowed_groups[0].to_bytes(), compressed);

    let mut writer = BinaryWriter::new();
    <Signer as Serializable>::serialize(&signer, &mut writer).unwrap();
    let mut expected = Vec::new();
    expected.extend_from_slice(&UInt160::zero().to_bytes());
    expected.push(WitnessScope::CUSTOM_GROUPS.bits());
    expected.push(1);
    expected.extend_from_slice(&compressed);
    assert_eq!(
        writer.into_bytes(),
        expected,
        "C# ECPoint.DeserializeFrom accepts uncompressed points, while ECPoint.Serialize writes compressed bytes"
    );
}
