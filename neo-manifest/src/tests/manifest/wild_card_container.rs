use super::*;
use neo_vm_rs::StackValue;

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

    assert_eq!(
        container.to_stack_value(),
        StackValue::Array(vec![
            StackValue::ByteString(b"transfer".to_vec()),
            StackValue::ByteString(b"balanceOf".to_vec()),
        ])
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
