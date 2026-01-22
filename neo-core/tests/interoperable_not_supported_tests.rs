use neo_core::ledger::BlockHeader;
use neo_core::network::p2p::payloads::{Signer, WitnessScope};
use neo_core::smart_contract::IInteroperable;
use neo_core::smart_contract::native::trimmed_block::TrimmedBlock;
use neo_core::smart_contract::notify_event_args::NotifyEventArgs;
use neo_core::UInt160;
use neo_vm::StackItem;

#[test]
#[should_panic(expected = "NotSupportedException: Signer::from_stack_item is not supported")]
fn signer_from_stack_item_panics() {
    let mut signer = Signer::new(UInt160::zero(), WitnessScope::NONE);
    signer.from_stack_item(StackItem::null());
}

#[test]
#[should_panic(
    expected = "NotSupportedException: FromStackItem is not supported for NotifyEventArgs"
)]
fn notify_event_args_from_stack_item_panics() {
    let mut args = NotifyEventArgs::new_with_optional_container(
        None,
        UInt160::zero(),
        "event".to_string(),
        Vec::new(),
    );
    args.from_stack_item(StackItem::null());
}

#[test]
#[should_panic(expected = "NotSupportedException: TrimmedBlock does not support FromStackItem")]
fn trimmed_block_from_stack_item_panics() {
    let mut block = TrimmedBlock::create(BlockHeader::default(), Vec::new());
    block.from_stack_item(StackItem::null());
}
