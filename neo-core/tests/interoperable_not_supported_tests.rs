use neo_core::ledger::BlockHeader;
use neo_core::network::p2p::payloads::{Signer, WitnessScope};
use neo_core::smart_contract::native::trimmed_block::TrimmedBlock;
use neo_core::smart_contract::notify_event_args::NotifyEventArgs;
use neo_core::smart_contract::IInteroperable;
use neo_core::UInt160;
use neo_vm::StackItem;

#[test]
fn signer_from_stack_item_returns_error() {
    let mut signer = Signer::new(UInt160::zero(), WitnessScope::NONE);
    let result = signer.from_stack_item(StackItem::null());
    assert!(
        result.is_err(),
        "Signer::from_stack_item should return Err for unsupported operation"
    );
}

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
