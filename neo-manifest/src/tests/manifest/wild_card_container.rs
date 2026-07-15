use super::*;
use neo_vm::StackItem;

fn stack_item_struct_eq(a: &neo_vm::StackItem, b: &neo_vm::StackItem) -> bool {
    a.equals(b).unwrap_or(false)
}

#[test]
fn string_wildcard_projects_to_neo_vm_null() {
    assert_eq!(
        WildCardContainer::<String>::create_wildcard().to_stack_item(),
        StackItem::Null
    );
}

#[test]
fn string_list_projects_to_neo_vm_byte_string_array() {
    let container = WildCardContainer::create(vec!["transfer".to_string(), "balanceOf".into()]);

    let left = container.to_stack_item();
    let right = StackItem::from_array(vec![
        StackItem::ByteString(b"transfer".to_vec()),
        StackItem::ByteString(b"balanceOf".to_vec()),
    ]);
    assert!(
        stack_item_struct_eq(&left, &right),
        "structural StackItem mismatch: {left:?} vs {right:?}"
    );
}

#[test]
fn string_interoperable_projection_matches_inherent_projection() {
    let container = WildCardContainer::create(vec!["deploy".to_string(), "update".into()]);
    let expected = container.to_stack_item();
    let projected = neo_vm::Interoperable::to_stack_item(&container).unwrap();

    assert!(stack_item_struct_eq(&projected, &expected));
}

#[test]
fn string_wildcard_reads_from_neo_vm_null() {
    assert_eq!(
        WildCardContainer::<String>::from_stack_item(&StackItem::Null).unwrap(),
        WildCardContainer::Wildcard
    );
}

#[test]
fn string_list_reads_from_neo_vm_array() {
    assert_eq!(
        WildCardContainer::<String>::from_stack_item(&StackItem::from_array(vec![
            StackItem::ByteString(b"mint".to_vec()),
            StackItem::ByteString(b"burn".to_vec()),
        ]))
        .unwrap(),
        WildCardContainer::create(vec!["mint".to_string(), "burn".into()])
    );
}

#[test]
fn string_list_rejects_struct_and_invalid_strings_like_csharp() {
    assert!(
        WildCardContainer::<String>::from_stack_item(&StackItem::from_struct(vec![
            StackItem::ByteString(b"verify".to_vec()),
            StackItem::ByteString(b"onNEP17Payment".to_vec()),
        ]))
        .is_err()
    );
    assert!(
        WildCardContainer::<String>::from_stack_item(&StackItem::from_array(vec![StackItem::Null]))
            .is_err()
    );
    assert!(
        WildCardContainer::<String>::from_stack_item(&StackItem::from_array(vec![
            StackItem::ByteString(vec![0xff])
        ]))
        .is_err()
    );
}
