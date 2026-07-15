use super::*;
use neo_vm::StackItem;

fn stack_item_struct_eq(a: &neo_vm::StackItem, b: &neo_vm::StackItem) -> bool {
    a.equals(b).unwrap_or(false)
}

fn parameter(name: &str, param_type: ContractParameterType) -> ContractParameterDefinition {
    ContractParameterDefinition::new(name.to_string(), param_type).unwrap()
}

#[test]
fn method_descriptor_projects_to_neo_vm_stack_item() {
    let method = ContractMethodDescriptor::new(
        "balanceOf".to_string(),
        vec![parameter("account", ContractParameterType::Hash160)],
        ContractParameterType::Integer,
        42,
        true,
    )
    .unwrap();

    let left = method.to_stack_item();
    let right = StackItem::from_struct(vec![
        StackItem::ByteString(b"balanceOf".to_vec()),
        StackItem::from_array(vec![StackItem::from_struct(vec![
            StackItem::ByteString(b"account".to_vec()),
            StackItem::from_i64(ContractParameterType::Hash160 as u8 as i64),
        ])]),
        StackItem::from_i64(ContractParameterType::Integer as u8 as i64),
        StackItem::from_i64(42),
        StackItem::Boolean(true),
    ]);
    assert!(
        stack_item_struct_eq(&left, &right),
        "structural StackItem mismatch: {left:?} vs {right:?}"
    );
}

#[test]
fn method_descriptor_reads_from_neo_vm_stack_item() {
    let mut method = ContractMethodDescriptor::default();

    method
        .from_stack_item(StackItem::from_struct(vec![
            StackItem::ByteString(b"symbol".to_vec()),
            StackItem::from_array(vec![StackItem::from_struct(vec![
                StackItem::ByteString(b"format".to_vec()),
                StackItem::from_i64(ContractParameterType::String as u8 as i64),
            ])]),
            StackItem::from_i64(ContractParameterType::String as u8 as i64),
            StackItem::from_i64(12),
            StackItem::Boolean(true),
        ]))
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
            .from_stack_item(StackItem::from_struct(vec![
                StackItem::ByteString(b"verify".to_vec()),
                StackItem::from_struct(vec![StackItem::from_struct(vec![
                    StackItem::ByteString(b"signature".to_vec()),
                    StackItem::from_i64(ContractParameterType::Signature as u8 as i64),
                ])]),
                StackItem::from_i64(ContractParameterType::Boolean as u8 as i64),
                StackItem::from_i64(5),
                StackItem::Boolean(false),
            ]))
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
            .from_stack_item(StackItem::from_struct(vec![
                StackItem::ByteString(b"changed".to_vec()),
                StackItem::from_array(Vec::new()),
                StackItem::from_i64(0x7f),
                StackItem::from_i64(3),
                StackItem::Boolean(true),
            ]))
            .is_err()
    );
    assert!(
        method
            .from_stack_item(StackItem::from_struct(vec![
                StackItem::Null,
                StackItem::from_array(Vec::new()),
                StackItem::from_i64(ContractParameterType::Boolean as u8 as i64),
                StackItem::from_i64(3),
                StackItem::Boolean(true),
            ]))
            .is_err()
    );
    assert!(
        method
            .from_stack_item(StackItem::from_struct(vec![
                StackItem::ByteString(vec![0xff]),
                StackItem::from_array(Vec::new()),
                StackItem::from_i64(ContractParameterType::Boolean as u8 as i64),
                StackItem::from_i64(3),
                StackItem::Boolean(true),
            ]))
            .is_err()
    );
    assert!(
        method
            .from_stack_item(StackItem::from_struct(vec![
                StackItem::ByteString(b"changed".to_vec()),
                StackItem::from_array(Vec::new()),
                StackItem::from_i64(ContractParameterType::Boolean as u8 as i64),
                StackItem::from_i64(i64::MAX),
                StackItem::Boolean(true),
            ]))
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
