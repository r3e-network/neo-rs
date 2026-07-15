use super::*;
use neo_vm::Interoperable;

/// Structural equality for stack items. Collection identity is not part of
/// serialized stack data, so structural equality is the correct notion for
/// round-trip and shape assertions.
fn stack_item_struct_eq(a: &StackItem, b: &StackItem) -> bool {
    a.equals(b).unwrap_or(false)
}

fn sample_notification() -> NotifyEventArgs {
    NotifyEventArgs::new_with_optional_container(
        None,
        UInt160::from_bytes(&[0x11; 20]).expect("script hash"),
        "Transfer".to_string(),
        vec![StackItem::from_i64(7)],
    )
}

#[test]
fn notify_event_projects_to_stack_item() {
    let notification = sample_notification();

    let left = notification.to_stack_item();
    let right = StackItem::from_array(vec![
        StackItem::from_byte_string(notification.script_hash.to_bytes()),
        StackItem::from_byte_string(b"Transfer".to_vec()),
        StackItem::from_array(vec![StackItem::from_i64(7)]),
    ]);
    assert!(
        stack_item_struct_eq(&left, &right),
        "structural StackItem mismatch: {left:?} vs {right:?}"
    );
}

#[test]
fn notify_event_prepared_state_projection_uses_stack_item_layout() {
    let notification = sample_notification();
    let prepared_state = StackItem::from_array(vec![StackItem::from_bool(true)]);

    let expected = StackItem::from_array(vec![
        StackItem::from_byte_string(notification.script_hash.to_bytes()),
        StackItem::from_byte_string(b"Transfer".to_vec()),
        prepared_state.clone(),
    ]);

    let projected = notification.to_stack_item_with_state_array(prepared_state.clone());
    assert!(
        stack_item_struct_eq(&projected, &expected),
        "structural StackItem mismatch: {projected:?} vs {expected:?}"
    );
    assert_eq!(
        notification
            .try_to_stack_item_with_state_array(prepared_state)
            .unwrap(),
        expected
    );
}

#[test]
fn notify_event_prepared_stack_item_state_preserves_readonly_flag() {
    let notification = sample_notification();
    let prepared_state = StackItem::from_array(vec![StackItem::from_i64(1)]);
    let StackItem::Array(array) = &prepared_state else {
        panic!("prepared state should be an array");
    };
    array.set_read_only(true);

    let projected = notification
        .try_to_stack_item_with_state_array(prepared_state)
        .expect("project notification");
    let StackItem::Array(notification_array) = projected else {
        panic!("notification projection should be an array");
    };
    let fields = notification_array.items();
    let StackItem::Array(state_array) = &fields[2] else {
        panic!("state projection should remain an array");
    };

    assert!(state_array.is_read_only());
}

#[test]
fn notify_event_interoperable_to_stack_item_matches_inherent() {
    let notification = sample_notification();
    let expected = notification.to_stack_item();

    let interop = Interoperable::to_stack_item(&notification).unwrap();
    assert!(
        stack_item_struct_eq(&interop, &expected),
        "structural StackItem mismatch: {interop:?} vs {expected:?}"
    );
}

#[test]
fn notify_event_state_and_retained_array_share_one_immutable_sequence() {
    let nested = StackItem::from_array(vec![StackItem::from_i64(1)]);
    let notification = NotifyEventArgs::new_with_optional_container(
        None,
        UInt160::from_bytes(&[0x22; 20]).expect("script hash"),
        "Transfer".to_string(),
        vec![nested],
    );

    let StackItem::Array(nested_from_state) = &notification.state()[0] else {
        panic!("notification state should contain an array");
    };
    nested_from_state
        .push(StackItem::from_i64(2))
        .expect("nested compounds remain reference values");

    let StackItem::Array(retained_state) = notification.state_array() else {
        panic!("retained state should be an array");
    };
    assert!(retained_state.is_read_only());
    let StackItem::Array(nested_from_retained) = &retained_state.items()[0] else {
        panic!("retained state should contain the same nested array");
    };
    assert_eq!(nested_from_retained.len(), 2);
}
