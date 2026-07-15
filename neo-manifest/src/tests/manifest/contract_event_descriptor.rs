use super::*;
use neo_primitives::ContractParameterType;
use neo_vm::StackItem;

fn stack_item_struct_eq(a: &neo_vm::StackItem, b: &neo_vm::StackItem) -> bool {
    a.equals(b).unwrap_or(false)
}

fn parameter(name: &str, param_type: ContractParameterType) -> ContractParameterDefinition {
    ContractParameterDefinition::new(name.to_string(), param_type).unwrap()
}

#[test]
fn event_descriptor_projects_to_neo_vm_stack_item() {
    let event = ContractEventDescriptor::new(
        "Transfer".to_string(),
        vec![
            parameter("from", ContractParameterType::Hash160),
            parameter("amount", ContractParameterType::Integer),
        ],
    )
    .unwrap();

    let left = event.to_stack_item();
    let right = StackItem::from_struct(vec![
        StackItem::ByteString(b"Transfer".to_vec()),
        StackItem::from_array(vec![
            StackItem::from_struct(vec![
                StackItem::ByteString(b"from".to_vec()),
                StackItem::from_i64(ContractParameterType::Hash160 as u8 as i64),
            ]),
            StackItem::from_struct(vec![
                StackItem::ByteString(b"amount".to_vec()),
                StackItem::from_i64(ContractParameterType::Integer as u8 as i64),
            ]),
        ]),
    ]);
    assert!(
        stack_item_struct_eq(&left, &right),
        "structural StackItem mismatch: {left:?} vs {right:?}"
    );
}

#[test]
fn event_descriptor_reads_from_neo_vm_stack_item() {
    let mut event = ContractEventDescriptor::default();

    event
        .from_stack_item(StackItem::from_struct(vec![
            StackItem::ByteString(b"Approval".to_vec()),
            StackItem::from_array(vec![StackItem::from_struct(vec![
                StackItem::ByteString(b"spender".to_vec()),
                StackItem::from_i64(ContractParameterType::Hash160 as u8 as i64),
            ])]),
        ]))
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
            .from_stack_item(StackItem::from_struct(vec![
                StackItem::ByteString(b"Vote".to_vec()),
                StackItem::from_struct(vec![StackItem::from_struct(vec![
                    StackItem::ByteString(b"candidate".to_vec()),
                    StackItem::from_i64(ContractParameterType::PublicKey as u8 as i64),
                ])]),
            ]))
            .is_err()
    );
}

#[test]
fn event_descriptor_rejects_invalid_name_like_csharp() {
    let mut event = ContractEventDescriptor::default();

    assert!(
        event
            .from_stack_item(StackItem::from_struct(vec![
                StackItem::Null,
                StackItem::from_array(Vec::new()),
            ]))
            .is_err()
    );
    assert!(
        event
            .from_stack_item(StackItem::from_struct(vec![
                StackItem::ByteString(vec![0xff]),
                StackItem::from_array(Vec::new()),
            ]))
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
