use super::*;
use neo_vm::StackValue;

fn stack_value_struct_eq(a: &neo_vm::StackValue, b: &neo_vm::StackValue) -> bool {
    a.structural_eq(b)
}

fn parameter(name: &str, param_type: ContractParameterType) -> ContractParameterDefinition {
    ContractParameterDefinition::new(name.to_string(), param_type).unwrap()
}

#[test]
fn method_descriptor_projects_to_neo_vm_rs_stack_value() {
    let method = ContractMethodDescriptor::new(
        "balanceOf".to_string(),
        vec![parameter("account", ContractParameterType::Hash160)],
        ContractParameterType::Integer,
        42,
        true,
    )
    .unwrap();

    let left = method.to_stack_value();
    let right = StackValue::Struct(
        neo_vm::next_stack_item_id(),
        vec![
            StackValue::ByteString(b"balanceOf".to_vec()),
            StackValue::Array(
                neo_vm::next_stack_item_id(),
                vec![StackValue::Struct(
                    neo_vm::next_stack_item_id(),
                    vec![
                        StackValue::ByteString(b"account".to_vec()),
                        StackValue::Integer(ContractParameterType::Hash160 as u8 as i64),
                    ],
                )],
            ),
            StackValue::Integer(ContractParameterType::Integer as u8 as i64),
            StackValue::Integer(42),
            StackValue::Boolean(true),
        ],
    );
    assert!(
        stack_value_struct_eq(&left, &right),
        "structural StackValue mismatch: {left:?} vs {right:?}"
    );
}

#[test]
fn method_descriptor_reads_from_neo_vm_rs_stack_value() {
    let mut method = ContractMethodDescriptor::default();

    method
        .from_stack_value(StackValue::Struct(
            neo_vm::next_stack_item_id(),
            vec![
                StackValue::ByteString(b"symbol".to_vec()),
                StackValue::Array(
                    neo_vm::next_stack_item_id(),
                    vec![StackValue::Struct(
                        neo_vm::next_stack_item_id(),
                        vec![
                            StackValue::ByteString(b"format".to_vec()),
                            StackValue::Integer(ContractParameterType::String as u8 as i64),
                        ],
                    )],
                ),
                StackValue::Integer(ContractParameterType::String as u8 as i64),
                StackValue::Integer(12),
                StackValue::Boolean(true),
            ],
        ))
        .unwrap();

    assert_eq!(method.name, "symbol");
    assert_eq!(
        method.parameters,
        vec![parameter("format", ContractParameterType::String)]
    );
    assert_eq!(method.return_type, ContractParameterType::String);
    assert_eq!(method.offset, 12);
    assert!(method.safe);
}

#[test]
fn method_descriptor_rejects_struct_parameter_sequence_like_csharp() {
    let mut method = ContractMethodDescriptor::default();

    assert!(
        method
            .from_stack_value(StackValue::Struct(
                neo_vm::next_stack_item_id(),
                vec![
                    StackValue::ByteString(b"verify".to_vec()),
                    StackValue::Struct(
                        neo_vm::next_stack_item_id(),
                        vec![StackValue::Struct(
                            neo_vm::next_stack_item_id(),
                            vec![
                                StackValue::ByteString(b"signature".to_vec()),
                                StackValue::Integer(ContractParameterType::Signature as u8 as i64),
                            ]
                        )]
                    ),
                    StackValue::Integer(ContractParameterType::Boolean as u8 as i64),
                    StackValue::Integer(5),
                    StackValue::Boolean(false),
                ]
            ))
            .is_err()
    );
}

#[test]
fn method_descriptor_rejects_invalid_stack_fields_like_csharp() {
    let mut method = ContractMethodDescriptor::new(
        "initial".to_string(),
        Vec::new(),
        ContractParameterType::Boolean,
        1,
        false,
    )
    .unwrap();

    assert!(
        method
            .from_stack_value(StackValue::Struct(
                neo_vm::next_stack_item_id(),
                vec![
                    StackValue::ByteString(b"changed".to_vec()),
                    StackValue::Array(neo_vm::next_stack_item_id(), Vec::new()),
                    StackValue::Integer(0x7f),
                    StackValue::Integer(3),
                    StackValue::Boolean(true),
                ]
            ))
            .is_err()
    );
    assert!(
        method
            .from_stack_value(StackValue::Struct(
                neo_vm::next_stack_item_id(),
                vec![
                    StackValue::Null,
                    StackValue::Array(neo_vm::next_stack_item_id(), Vec::new()),
                    StackValue::Integer(ContractParameterType::Boolean as u8 as i64),
                    StackValue::Integer(3),
                    StackValue::Boolean(true),
                ]
            ))
            .is_err()
    );
    assert!(
        method
            .from_stack_value(StackValue::Struct(
                neo_vm::next_stack_item_id(),
                vec![
                    StackValue::ByteString(vec![0xff]),
                    StackValue::Array(neo_vm::next_stack_item_id(), Vec::new()),
                    StackValue::Integer(ContractParameterType::Boolean as u8 as i64),
                    StackValue::Integer(3),
                    StackValue::Boolean(true),
                ]
            ))
            .is_err()
    );
    assert!(
        method
            .from_stack_value(StackValue::Struct(
                neo_vm::next_stack_item_id(),
                vec![
                    StackValue::ByteString(b"changed".to_vec()),
                    StackValue::Array(neo_vm::next_stack_item_id(), Vec::new()),
                    StackValue::Integer(ContractParameterType::Boolean as u8 as i64),
                    StackValue::Integer(i64::MAX),
                    StackValue::Boolean(true),
                ]
            ))
            .is_err()
    );
}

#[test]
fn method_descriptor_from_json_rejects_missing_or_invalid_fields_like_csharp() {
    let missing_return_type = serde_json::json!({
        "name": "main",
        "parameters": [],
        "offset": 0,
        "safe": false
    });
    assert!(ContractMethodDescriptor::from_json(&missing_return_type).is_err());

    let invalid_parameter = serde_json::json!({
        "name": "main",
        "parameters": [{"name": "bad", "type": "Void"}],
        "returntype": "Void",
        "offset": 0,
        "safe": false
    });
    assert!(ContractMethodDescriptor::from_json(&invalid_parameter).is_err());

    let overflowing_offset = serde_json::json!({
        "name": "main",
        "parameters": [],
        "returntype": "Void",
        "offset": i64::from(i32::MAX) + 1,
        "safe": false
    });
    assert!(ContractMethodDescriptor::from_json(&overflowing_offset).is_err());

    let alias_return_type = serde_json::json!({
        "name": "main",
        "parameters": [],
        "returntype": "INT",
        "offset": 0,
        "safe": false
    });
    assert!(ContractMethodDescriptor::from_json(&alias_return_type).is_err());
}

#[test]
fn method_descriptor_from_json_accepts_csharp_numeric_return_type() {
    let numeric_void = serde_json::json!({
        "name": "main",
        "parameters": [],
        "returntype": "255",
        "offset": 0,
        "safe": false
    });
    let method = ContractMethodDescriptor::from_json(&numeric_void).unwrap();
    assert_eq!(method.return_type, ContractParameterType::Void);
}
