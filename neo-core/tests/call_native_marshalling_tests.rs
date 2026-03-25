use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_vm::StackItem;

#[test]
fn stack_item_to_bytes_serializes_null_as_binary_null_payload() {
    let encoded = ApplicationEngine::stack_item_to_bytes(StackItem::null())
        .expect("null stack item should marshal");
    assert_eq!(
        encoded,
        vec![0],
        "native ByteArray marshalling should preserve serialized null payload",
    );
}
