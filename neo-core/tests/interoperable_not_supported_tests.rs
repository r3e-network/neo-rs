use neo_core::UInt160;
use neo_core::neo_vm::StackItem;
use neo_core::smart_contract::Interoperable;
use neo_core::smart_contract::NotifyEventArgs;
use neo_core::smart_contract::native::trimmed_block::TrimmedBlock;
use neo_protocol::execution::BlockHeader;

#[test]
fn notify_event_args_from_stack_item_returns_error() {
    let mut args = NotifyEventArgs::new_with_optional_container(
        None,
        UInt160::zero(),
        "event".to_string(),
        Vec::new(),
    );
    let result = args.from_stack_item(StackItem::null());
    assert!(
        result.is_err(),
        "NotifyEventArgs::from_stack_item should return Err"
    );
}

#[test]
fn trimmed_block_from_stack_item_returns_error() {
    let mut block = TrimmedBlock::create(BlockHeader::default(), Vec::new());
    let result = block.from_stack_item(StackItem::null());
    assert!(
        result.is_err(),
        "TrimmedBlock::from_stack_item should return Err"
    );
}
