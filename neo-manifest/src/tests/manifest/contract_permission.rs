use super::*;
use neo_vm_rs::StackValue;

/// Structural equality for StackValue that ignores the reference-identity ids
/// on compound variants. Collection identity is not part of serialized
/// stack data, so structural equality is the correct notion for round-trip / shape
/// assertions.
fn stack_value_struct_eq(a: &neo_vm_rs::StackValue, b: &neo_vm_rs::StackValue) -> bool {
    use neo_vm_rs::StackValue::*;
    match (a, b) {
        (Buffer(x), Buffer(y)) => x == y,
        (Array(x), Array(y)) | (Struct(x), Struct(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| stack_value_struct_eq(p, q))
        }
        (Map(x), Map(y)) => {
            x.len() == y.len()
                && x.iter().zip(y).all(|((k1, v1), (k2, v2))| {
                    stack_value_struct_eq(k1, k2) && stack_value_struct_eq(v1, v2)
                })
        }
        _ => a == b,
    }
}

#[test]
fn contract_permission_projects_to_neo_vm_rs_stack_value() {
    let hash = UInt160::from_bytes(&[0x44; 20]).expect("hash");
    let permission = ContractPermission::for_contract(
        hash,
        WildCardContainer::create(vec!["transfer".to_string(), "balanceOf".into()]),
    );

    let left = permission.to_stack_value();
    let right = StackValue::Struct(vec![
        StackValue::ByteString(hash.to_bytes()),
        StackValue::Array(vec![
            StackValue::ByteString(b"transfer".to_vec()),
            StackValue::ByteString(b"balanceOf".to_vec()),
        ]),
    ]);
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
        .from_stack_value(StackValue::Struct(vec![
            StackValue::ByteString(hash.to_bytes()),
            StackValue::Array(vec![StackValue::ByteString(b"mint".to_vec())]),
        ]))
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
