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

    let left = abi.to_stack_value();
    let right = StackValue::Struct(
        neo_vm_rs::next_stack_item_id(),
        vec![
            StackValue::Array(
                neo_vm_rs::next_stack_item_id(),
                vec![StackValue::Struct(
                    neo_vm_rs::next_stack_item_id(),
                    vec![
                        StackValue::ByteString(b"main".to_vec()),
                        StackValue::Array(neo_vm_rs::next_stack_item_id(), Vec::new()),
                        StackValue::Integer(ContractParameterType::Void as u8 as i64),
                        StackValue::Integer(7),
                        StackValue::Boolean(true),
                    ],
                )],
            ),
            StackValue::Array(
                neo_vm_rs::next_stack_item_id(),
                vec![StackValue::Struct(
                    neo_vm_rs::next_stack_item_id(),
                    vec![
                        StackValue::ByteString(b"Notify".to_vec()),
                        StackValue::Array(neo_vm_rs::next_stack_item_id(), Vec::new()),
                    ],
                )],
            ),
        ],
    );
    assert!(
        stack_value_struct_eq(&left, &right),
        "structural StackValue mismatch: {left:?} vs {right:?}"
    );
}

#[test]
fn contract_abi_reads_from_neo_vm_rs_stack_value_and_clears_method_cache() {
    let mut abi = ContractAbi::new(vec![method("old")], Vec::new());
    assert!(abi.get_method("old", 0).is_some());

    abi.from_stack_value(StackValue::Struct(
        neo_vm_rs::next_stack_item_id(),
        vec![
            StackValue::Array(
                neo_vm_rs::next_stack_item_id(),
                vec![method("new").to_stack_value()],
            ),
            StackValue::Array(
                neo_vm_rs::next_stack_item_id(),
                vec![event("Updated").to_stack_value()],
            ),
        ],
    ))
    .unwrap();

    assert!(abi.get_method("old", 0).is_none());
    assert!(abi.get_method("new", 0).is_some());
    assert_eq!(abi.events, vec![event("Updated")]);
}

#[test]
fn contract_abi_rejects_struct_sequences_like_csharp() {
    let mut abi = ContractAbi::default();

    assert!(
        abi.from_stack_value(StackValue::Struct(
            neo_vm_rs::next_stack_item_id(),
            vec![
                StackValue::Struct(
                    neo_vm_rs::next_stack_item_id(),
                    vec![method("main").to_stack_value()]
                ),
                StackValue::Array(
                    neo_vm_rs::next_stack_item_id(),
                    vec![event("Notify").to_stack_value()]
                ),
            ]
        ))
        .is_err()
    );
    assert!(
        abi.from_stack_value(StackValue::Struct(
            neo_vm_rs::next_stack_item_id(),
            vec![
                StackValue::Array(
                    neo_vm_rs::next_stack_item_id(),
                    vec![method("main").to_stack_value()]
                ),
                StackValue::Struct(
                    neo_vm_rs::next_stack_item_id(),
                    vec![event("Notify").to_stack_value()]
                ),
            ]
        ))
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
