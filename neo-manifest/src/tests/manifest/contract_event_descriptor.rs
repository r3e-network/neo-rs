use super::*;
use neo_primitives::ContractParameterType;
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

fn parameter(name: &str, param_type: ContractParameterType) -> ContractParameterDefinition {
    ContractParameterDefinition::new(name.to_string(), param_type).unwrap()
}

#[test]
fn event_descriptor_projects_to_neo_vm_rs_stack_value() {
    let event = ContractEventDescriptor::new(
        "Transfer".to_string(),
        vec![
            parameter("from", ContractParameterType::Hash160),
            parameter("amount", ContractParameterType::Integer),
        ],
    )
    .unwrap();

    let left = event.to_stack_value();
    let right = StackValue::Struct(
        neo_vm_rs::next_stack_item_id(),
        vec![
            StackValue::ByteString(b"Transfer".to_vec()),
            StackValue::Array(
                neo_vm_rs::next_stack_item_id(),
                vec![
                    StackValue::Struct(
                        neo_vm_rs::next_stack_item_id(),
                        vec![
                            StackValue::ByteString(b"from".to_vec()),
                            StackValue::Integer(ContractParameterType::Hash160 as u8 as i64),
                        ],
                    ),
                    StackValue::Struct(
                        neo_vm_rs::next_stack_item_id(),
                        vec![
                            StackValue::ByteString(b"amount".to_vec()),
                            StackValue::Integer(ContractParameterType::Integer as u8 as i64),
                        ],
                    ),
                ],
            ),
        ],
    );
    assert!(
        stack_value_struct_eq(&left, &right),
        "structural StackValue mismatch: {left:?} vs {right:?}"
    );
}

#[test]
fn event_descriptor_reads_from_neo_vm_rs_stack_value() {
    let mut event = ContractEventDescriptor::default();

    event
        .from_stack_value(StackValue::Struct(
            neo_vm_rs::next_stack_item_id(),
            vec![
                StackValue::ByteString(b"Approval".to_vec()),
                StackValue::Array(
                    neo_vm_rs::next_stack_item_id(),
                    vec![StackValue::Struct(
                        neo_vm_rs::next_stack_item_id(),
                        vec![
                            StackValue::ByteString(b"spender".to_vec()),
                            StackValue::Integer(ContractParameterType::Hash160 as u8 as i64),
                        ],
                    )],
                ),
            ],
        ))
        .unwrap();

    assert_eq!(event.name, "Approval");
    assert_eq!(
        event.parameters,
        vec![parameter("spender", ContractParameterType::Hash160)]
    );
}

#[test]
fn event_descriptor_rejects_struct_parameter_sequence_like_csharp() {
    let mut event = ContractEventDescriptor::default();

    assert!(
        event
            .from_stack_value(StackValue::Struct(
                neo_vm_rs::next_stack_item_id(),
                vec![
                    StackValue::ByteString(b"Vote".to_vec()),
                    StackValue::Struct(
                        neo_vm_rs::next_stack_item_id(),
                        vec![StackValue::Struct(
                            neo_vm_rs::next_stack_item_id(),
                            vec![
                                StackValue::ByteString(b"candidate".to_vec()),
                                StackValue::Integer(ContractParameterType::PublicKey as u8 as i64),
                            ]
                        )]
                    ),
                ]
            ))
            .is_err()
    );
}

#[test]
fn event_descriptor_rejects_invalid_name_like_csharp() {
    let mut event = ContractEventDescriptor::default();

    assert!(
        event
            .from_stack_value(StackValue::Struct(
                neo_vm_rs::next_stack_item_id(),
                vec![
                    StackValue::Null,
                    StackValue::Array(neo_vm_rs::next_stack_item_id(), Vec::new()),
                ]
            ))
            .is_err()
    );
    assert!(
        event
            .from_stack_value(StackValue::Struct(
                neo_vm_rs::next_stack_item_id(),
                vec![
                    StackValue::ByteString(vec![0xff]),
                    StackValue::Array(neo_vm_rs::next_stack_item_id(), Vec::new()),
                ]
            ))
            .is_err()
    );
}

#[test]
fn event_descriptor_from_json_rejects_missing_or_invalid_parameters_like_csharp() {
    let missing_parameters = serde_json::json!({
        "name": "Notify"
    });
    assert!(ContractEventDescriptor::from_json(&missing_parameters).is_err());

    let invalid_parameter = serde_json::json!({
        "name": "Notify",
        "parameters": [{"name": "bad", "type": "Void"}]
    });
    assert!(ContractEventDescriptor::from_json(&invalid_parameter).is_err());
}
