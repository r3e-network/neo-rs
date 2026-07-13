use super::*;
use neo_vm_rs::StackValue;

fn stack_value_struct_eq(a: &neo_vm_rs::StackValue, b: &neo_vm_rs::StackValue) -> bool {
    a.structural_eq(b)
}

#[test]
fn contract_permission_projects_to_neo_vm_rs_stack_value() {
    let hash = UInt160::from_bytes(&[0x44; 20]).expect("hash");
    let permission = ContractPermission::for_contract(
        hash,
        WildCardContainer::create(vec!["transfer".to_string(), "balanceOf".into()]),
    );

    let left = permission.to_stack_value();
    let right = StackValue::Struct(
        neo_vm_rs::next_stack_item_id(),
        vec![
            StackValue::ByteString(hash.to_bytes()),
            StackValue::Array(
                neo_vm_rs::next_stack_item_id(),
                vec![
                    StackValue::ByteString(b"transfer".to_vec()),
                    StackValue::ByteString(b"balanceOf".to_vec()),
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
fn contract_permission_reads_from_neo_vm_rs_stack_value() {
    let hash = UInt160::from_bytes(&[0x55; 20]).expect("hash");
    let mut permission = ContractPermission::default_wildcard();

    permission
        .from_stack_value(StackValue::Struct(
            neo_vm_rs::next_stack_item_id(),
            vec![
                StackValue::ByteString(hash.to_bytes()),
                StackValue::Array(
                    neo_vm_rs::next_stack_item_id(),
                    vec![StackValue::ByteString(b"mint".to_vec())],
                ),
            ],
        ))
        .unwrap();

    assert_eq!(
        permission.contract,
        ContractPermissionDescriptor::Hash(hash)
    );
    assert_eq!(
        permission.methods,
        WildCardContainer::create(vec!["mint".to_string()])
    );
}

#[test]
fn contract_permission_from_json_allows_empty_method_list_like_csharp() {
    let json = serde_json::json!({
        "contract": "*",
        "methods": []
    });

    let permission = ContractPermission::from_json(&json).unwrap();
    assert_eq!(permission.contract, ContractPermissionDescriptor::Wildcard);
    assert_eq!(
        permission.methods,
        WildCardContainer::create(Vec::<String>::new())
    );
    assert!(permission.validate().is_ok());
}

#[test]
fn contract_permission_from_json_rejects_empty_or_duplicate_methods_like_csharp() {
    let empty_method = serde_json::json!({
        "contract": "*",
        "methods": [""]
    });
    assert!(ContractPermission::from_json(&empty_method).is_err());

    let duplicate_method = serde_json::json!({
        "contract": "*",
        "methods": ["mint", "mint"]
    });
    assert!(ContractPermission::from_json(&duplicate_method).is_err());
}
