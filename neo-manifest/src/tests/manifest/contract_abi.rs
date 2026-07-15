use super::*;
use neo_primitives::ContractParameterType;
use neo_vm::StackItem;

fn stack_item_struct_eq(a: &neo_vm::StackItem, b: &neo_vm::StackItem) -> bool {
    a.equals(b).unwrap_or(false)
}

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
fn contract_abi_projects_to_neo_vm_stack_item() {
    let abi = ContractAbi::new(vec![method("main")], vec![event("Notify")]);

    let left = abi.to_stack_item();
    let right = StackItem::from_struct(vec![
        StackItem::from_array(vec![StackItem::from_struct(vec![
            StackItem::ByteString(b"main".to_vec()),
            StackItem::from_array(Vec::new()),
            StackItem::from_i64(ContractParameterType::Void as u8 as i64),
            StackItem::from_i64(7),
            StackItem::Boolean(true),
        ])]),
        StackItem::from_array(vec![StackItem::from_struct(vec![
            StackItem::ByteString(b"Notify".to_vec()),
            StackItem::from_array(Vec::new()),
        ])]),
    ]);
    assert!(
        stack_item_struct_eq(&left, &right),
        "structural StackItem mismatch: {left:?} vs {right:?}"
    );
}

#[test]
fn contract_abi_reads_from_neo_vm_stack_item_and_clears_method_cache() {
    let mut abi = ContractAbi::new(vec![method("old")], Vec::new());
    assert!(abi.get_method("old", 0).is_some());

    abi.from_stack_item(StackItem::from_struct(vec![
        StackItem::from_array(vec![method("new").to_stack_item()]),
        StackItem::from_array(vec![event("Updated").to_stack_item()]),
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
        abi.from_stack_item(StackItem::from_struct(vec![
            StackItem::from_struct(vec![method("main").to_stack_item()]),
            StackItem::from_array(vec![event("Notify").to_stack_item()]),
        ]))
        .is_err()
    );
    assert!(
        abi.from_stack_item(StackItem::from_struct(vec![
            StackItem::from_array(vec![method("main").to_stack_item()]),
            StackItem::from_struct(vec![event("Notify").to_stack_item()]),
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
