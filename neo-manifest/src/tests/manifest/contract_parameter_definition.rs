use super::*;
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
fn parameter_definition_projects_to_neo_vm_rs_stack_value() {
    let definition =
        ContractParameterDefinition::new("owner".to_string(), ContractParameterType::Hash160)
            .unwrap();

    let left = definition.to_stack_value();
    let right = StackValue::Struct(
        neo_vm_rs::next_stack_item_id(),
        vec![
            StackValue::ByteString(b"owner".to_vec()),
            StackValue::Integer(ContractParameterType::Hash160 as u8 as i64),
        ],
    );
    assert!(
        stack_value_struct_eq(&left, &right),
        "structural StackValue mismatch: {left:?} vs {right:?}"
    );
}

#[test]
fn parameter_definition_reads_from_neo_vm_rs_stack_value() {
    let mut definition = ContractParameterDefinition::default();

    definition
        .from_stack_value(StackValue::Struct(
            neo_vm_rs::next_stack_item_id(),
            vec![
                StackValue::ByteString(b"flag".to_vec()),
                StackValue::Integer(ContractParameterType::Boolean as u8 as i64),
            ],
        ))
        .unwrap();

    assert_eq!(definition.name, "flag");
    assert_eq!(definition.param_type, ContractParameterType::Boolean);
}

#[test]
fn parameter_definition_rejects_invalid_stack_fields_like_csharp() {
    let mut definition =
        ContractParameterDefinition::new("initial".to_string(), ContractParameterType::String)
            .unwrap();

    assert!(
        definition
            .from_stack_value(StackValue::Struct(
                neo_vm_rs::next_stack_item_id(),
                vec![
                    StackValue::ByteString(b"changed".to_vec()),
                    StackValue::Integer(0x7f),
                ]
            ))
            .is_err()
    );
    assert!(
        definition
            .from_stack_value(StackValue::Struct(
                neo_vm_rs::next_stack_item_id(),
                vec![
                    StackValue::Null,
                    StackValue::Integer(ContractParameterType::Boolean as u8 as i64),
                ]
            ))
            .is_err()
    );
    assert!(
        definition
            .from_stack_value(StackValue::Struct(
                neo_vm_rs::next_stack_item_id(),
                vec![
                    StackValue::ByteString(vec![0xff]),
                    StackValue::Integer(ContractParameterType::Boolean as u8 as i64),
                ]
            ))
            .is_err()
    );
    assert!(
        definition
            .from_stack_value(StackValue::Struct(
                neo_vm_rs::next_stack_item_id(),
                vec![
                    StackValue::ByteString(b"changed".to_vec()),
                    StackValue::Integer(-1),
                ]
            ))
            .is_err()
    );
}

#[test]
fn parameter_definition_from_json_uses_csharp_enum_parse_rules() {
    let numeric_boolean = serde_json::json!({
        "name": "flag",
        "type": "16"
    });
    let definition = ContractParameterDefinition::from_json(&numeric_boolean).unwrap();
    assert_eq!(definition.param_type, ContractParameterType::Boolean);

    let lowercase_alias = serde_json::json!({
        "name": "flag",
        "type": "bool"
    });
    assert!(ContractParameterDefinition::from_json(&lowercase_alias).is_err());

    let case_variant = serde_json::json!({
        "name": "flag",
        "type": "boolean"
    });
    assert!(ContractParameterDefinition::from_json(&case_variant).is_err());
}
