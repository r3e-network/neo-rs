use neo_core::UInt160;
use neo_core::neo_vm::StackItem;
use neo_core::smart_contract::{Interoperable, NotifyEventArgs};

#[test]
fn notify_event_args_to_stack_item_deep_copies_state() {
    let inner_array = StackItem::from_array(vec![StackItem::from_int(1)]);
    let args = NotifyEventArgs::new_with_optional_container(
        None,
        UInt160::zero(),
        "event".to_string(),
        vec![inner_array],
    );

    let outer = args.to_stack_item().unwrap();
    let state_item = match outer {
        StackItem::Array(array) => array.get(2).expect("state item"),
        _ => panic!("expected array from NotifyEventArgs::to_stack_item"),
    };

    let StackItem::Array(state_array) = state_item else {
        panic!("expected array state in NotifyEventArgs::to_stack_item")
    };
    state_array
        .push(StackItem::from_int(2))
        .expect("push into state array");

    let StackItem::Array(original_array) = &args.state[0] else {
        panic!("expected array state in NotifyEventArgs")
    };
    assert_eq!(original_array.items().len(), 1);
}
