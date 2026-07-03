use super::*;
use neo_vm::Interoperable;

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

    let left = notification.to_stack_value().expect("stack value");
    let right = StackValue::Array(
        neo_vm_rs::next_stack_item_id(),
        vec![
            StackValue::ByteString(notification.script_hash.to_bytes()),
            StackValue::ByteString(b"Transfer".to_vec()),
            StackValue::Array(
                neo_vm_rs::next_stack_item_id(),
                vec![StackValue::Integer(7)],
            ),
        ],
    );
    assert!(
        stack_value_struct_eq(&left, &right),
        "structural StackValue mismatch: {left:?} vs {right:?}"
    );
}

#[test]
fn notify_event_prepared_state_projection_uses_stack_value_layout() {
    let notification = sample_notification();
    let prepared_state = StackValue::Array(
        neo_vm_rs::next_stack_item_id(),
        vec![StackValue::Boolean(true)],
    );

    let expected = StackValue::Array(
        neo_vm_rs::next_stack_item_id(),
        vec![
            StackValue::ByteString(notification.script_hash.to_bytes()),
            StackValue::ByteString(b"Transfer".to_vec()),
            prepared_state.clone(),
        ],
    );

    let projected = notification.to_stack_value_with_state_array(prepared_state.clone());
    assert!(
        stack_value_struct_eq(&projected, &expected),
        "structural StackValue mismatch: {projected:?} vs {expected:?}"
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

    let interop = Interoperable::to_stack_value(&notification).unwrap();
    assert!(
        stack_value_struct_eq(&interop, &expected),
        "structural StackValue mismatch: {interop:?} vs {expected:?}"
    );
}
