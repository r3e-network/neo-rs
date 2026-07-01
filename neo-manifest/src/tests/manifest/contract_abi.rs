use super::*;
use neo_primitives::ContractParameterType;
use neo_vm_rs::StackValue;

fn method(name: &str) -> ContractMethodDescriptor {
    ContractMethodDescriptor::new(
        name.to_string(),
        Vec::new(),
        ContractParameterType::Void,
        7,
        true,
    )
    .unwrap()
}

fn event(name: &str) -> ContractEventDescriptor {
    ContractEventDescriptor::new(name.to_string(), Vec::new()).unwrap()
}

#[test]
fn contract_abi_projects_to_neo_vm_rs_stack_value() {
    let abi = ContractAbi::new(vec![method("main")], vec![event("Notify")]);

    assert_eq!(
        abi.to_stack_value(),
        StackValue::Struct(vec![
            StackValue::Array(vec![StackValue::Struct(vec![
                StackValue::ByteString(b"main".to_vec()),
                StackValue::Array(Vec::new()),
                StackValue::Integer(ContractParameterType::Void as u8 as i64),
                StackValue::Integer(7),
                StackValue::Boolean(true),
            ])]),
            StackValue::Array(vec![StackValue::Struct(vec![
                StackValue::ByteString(b"Notify".to_vec()),
                StackValue::Array(Vec::new()),
            ])]),
        ])
    );
}

#[test]
fn contract_abi_reads_from_neo_vm_rs_stack_value_and_clears_method_cache() {
    let mut abi = ContractAbi::new(vec![method("old")], Vec::new());
    assert!(abi.get_method("old", 0).is_some());

    abi.from_stack_value(StackValue::Struct(vec![
        StackValue::Array(vec![method("new").to_stack_value()]),
        StackValue::Array(vec![event("Updated").to_stack_value()]),
    ]))
    .unwrap();

    assert!(abi.get_method("old", 0).is_none());
    assert!(abi.get_method("new", 0).is_some());
    assert_eq!(abi.events, vec![event("Updated")]);
}

#[test]
fn contract_abi_rejects_struct_sequences_like_csharp() {
    let mut abi = ContractAbi::default();

    assert!(
        abi.from_stack_value(StackValue::Struct(vec![
            StackValue::Struct(vec![method("main").to_stack_value()]),
            StackValue::Array(vec![event("Notify").to_stack_value()]),
        ]))
        .is_err()
    );
    assert!(
        abi.from_stack_value(StackValue::Struct(vec![
            StackValue::Array(vec![method("main").to_stack_value()]),
            StackValue::Struct(vec![event("Notify").to_stack_value()]),
        ]))
        .is_err()
    );
}

#[test]
fn contract_abi_from_json_rejects_malformed_children_like_csharp() {
    let invalid_method = serde_json::json!({
        "methods": [{
            "name": "broken",
            "parameters": [{"name": "bad", "type": "Void"}],
            "returntype": "Void",
            "offset": 0,
            "safe": false
        }],
        "events": []
    });
    assert!(ContractAbi::from_json(&invalid_method).is_err());

    let invalid_event = serde_json::json!({
        "methods": [{
            "name": "main",
            "parameters": [],
            "returntype": "Void",
            "offset": 0,
            "safe": false
        }],
        "events": [{
            "name": "Notify",
            "parameters": [{"name": "", "type": "String"}]
        }]
    });
    assert!(ContractAbi::from_json(&invalid_event).is_err());

    let non_array = serde_json::json!({
        "methods": {},
        "events": []
    });
    assert!(ContractAbi::from_json(&non_array).is_err());
}
