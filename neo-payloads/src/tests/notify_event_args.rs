use super::*;
use neo_vm::Interoperable;

fn sample_notification() -> NotifyEventArgs {
    NotifyEventArgs::new_with_optional_container(
        None,
        UInt160::from_bytes(&[0x11; 20]).expect("script hash"),
        "Transfer".to_string(),
        vec![StackItem::from_i64(7)],
    )
}

#[test]
fn notify_event_projects_to_neo_vm_rs_stack_value() {
    let notification = sample_notification();

    assert_eq!(
        notification.to_stack_value().expect("stack value"),
        StackValue::Array(
            0,
            vec![
                StackValue::ByteString(notification.script_hash.to_bytes()),
                StackValue::ByteString(b"Transfer".to_vec()),
                StackValue::Array(0, vec![StackValue::Integer(7)]),
            ]
        )
    );
}

#[test]
fn notify_event_prepared_state_projection_uses_stack_value_layout() {
    let notification = sample_notification();
    let prepared_state = StackValue::Array(0, vec![StackValue::Boolean(true)]);

    let expected = StackValue::Array(
        0,
        vec![
            StackValue::ByteString(notification.script_hash.to_bytes()),
            StackValue::ByteString(b"Transfer".to_vec()),
            prepared_state.clone(),
        ],
    );

    assert_eq!(
        notification.to_stack_value_with_state_array(prepared_state.clone()),
        expected
    );
    assert_eq!(
        notification
            .try_to_stack_item_with_state_array(StackItem::try_from(prepared_state).unwrap())
            .unwrap(),
        StackItem::try_from(expected).unwrap()
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
fn notify_event_interoperable_to_stack_value_matches_inherent() {
    let notification = sample_notification();
    let expected = notification.to_stack_value().unwrap();

    assert_eq!(
        Interoperable::to_stack_value(&notification).unwrap(),
        expected
    );
}
