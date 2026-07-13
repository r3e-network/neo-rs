use super::*;
use neo_primitives::ContractParameterType;
use neo_vm::StackValue;

fn stack_value_struct_eq(a: &neo_vm::StackValue, b: &neo_vm::StackValue) -> bool {
    a.structural_eq(b)
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
        neo_vm::next_stack_item_id(),
        vec![
            StackValue::ByteString(b"Transfer".to_vec()),
            StackValue::Array(
                neo_vm::next_stack_item_id(),
                vec![
                    StackValue::Struct(
                        neo_vm::next_stack_item_id(),
                        vec![
                            StackValue::ByteString(b"from".to_vec()),
                            StackValue::Integer(ContractParameterType::Hash160 as u8 as i64),
                        ],
                    ),
                    StackValue::Struct(
                        neo_vm::next_stack_item_id(),
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
            neo_vm::next_stack_item_id(),
            vec![
                StackValue::ByteString(b"Approval".to_vec()),
                StackValue::Array(
                    neo_vm::next_stack_item_id(),
                    vec![StackValue::Struct(
                        neo_vm::next_stack_item_id(),
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
                neo_vm::next_stack_item_id(),
                vec![
                    StackValue::ByteString(b"Vote".to_vec()),
                    StackValue::Struct(
                        neo_vm::next_stack_item_id(),
                        vec![StackValue::Struct(
                            neo_vm::next_stack_item_id(),
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
                neo_vm::next_stack_item_id(),
                vec![
                    StackValue::Null,
                    StackValue::Array(neo_vm::next_stack_item_id(), Vec::new()),
                ]
            ))
            .is_err()
    );
    assert!(
        event
            .from_stack_value(StackValue::Struct(
                neo_vm::next_stack_item_id(),
                vec![
                    StackValue::ByteString(vec![0xff]),
                    StackValue::Array(neo_vm::next_stack_item_id(), Vec::new()),
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
