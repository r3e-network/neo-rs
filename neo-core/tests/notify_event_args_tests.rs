use neo_core::UInt160;
use neo_core::smart_contract::{IInteroperable, NotifyEventArgs};
use neo_vm::StackItem;

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

    let state_array = match state_item {
        StackItem::Array(array) => array,
        _ => panic!("expected array state in NotifyEventArgs::to_stack_item"),
    };
    state_array
        .push(StackItem::from_int(2))
        .expect("push into state array");

    let original_array = match &args.state[0] {
        StackItem::Array(array) => array,
        _ => panic!("expected array state in NotifyEventArgs"),
    };
    assert_eq!(original_array.items().len(), 1);
}
