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
fn string_wildcard_projects_to_neo_vm_rs_null() {
    assert_eq!(
        WildCardContainer::<String>::create_wildcard().to_stack_value(),
        StackValue::Null
    );
}

#[test]
fn string_list_projects_to_neo_vm_rs_byte_string_array() {
    let container = WildCardContainer::create(vec!["transfer".to_string(), "balanceOf".into()]);

    let left = container.to_stack_value();
    let right = StackValue::Array(vec![
        StackValue::ByteString(b"transfer".to_vec()),
        StackValue::ByteString(b"balanceOf".to_vec()),
    ]);
    assert!(
        stack_value_struct_eq(&left, &right),
        "structural StackValue mismatch: {left:?} vs {right:?}"
    );
}

#[test]
fn string_stack_item_projection_matches_stack_value_projection() {
    let container = WildCardContainer::create(vec!["deploy".to_string(), "update".into()]);
    let expected = StackItem::try_from(container.to_stack_value()).unwrap();

    assert_eq!(container.to_stack_item(), expected);
}

#[test]
fn string_wildcard_reads_from_neo_vm_rs_null() {
    assert_eq!(
        WildCardContainer::<String>::from_stack_value(StackValue::Null).unwrap(),
        WildCardContainer::Wildcard
    );
}

#[test]
fn string_list_reads_from_neo_vm_rs_array() {
    assert_eq!(
        WildCardContainer::<String>::from_stack_value(StackValue::Array(vec![
            StackValue::ByteString(b"mint".to_vec()),
            StackValue::ByteString(b"burn".to_vec()),
        ]))
        .unwrap(),
        WildCardContainer::create(vec!["mint".to_string(), "burn".into()])
    );
}

#[test]
fn string_list_rejects_struct_and_invalid_strings_like_csharp() {
    assert!(
        WildCardContainer::<String>::from_stack_value(StackValue::Struct(vec![
            StackValue::ByteString(b"verify".to_vec()),
            StackValue::ByteString(b"onNEP17Payment".to_vec()),
        ]))
        .is_err()
    );
    assert!(
        WildCardContainer::<String>::from_stack_value(StackValue::Array(vec![StackValue::Null]))
            .is_err()
    );
    assert!(
        WildCardContainer::<String>::from_stack_value(StackValue::Array(vec![
            StackValue::ByteString(vec![0xff])
        ]))
        .is_err()
    );
}
