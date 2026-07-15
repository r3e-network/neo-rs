//! Stack-item decoding helpers for persisted native-contract records.

use anyhow::{Result, anyhow};
use neo_serialization::BinarySerializer;
use neo_vm::{ExecutionEngineLimits, StackItem};
use num_traits::ToPrimitive;

pub(super) fn deserialize_stack_item(bytes: &[u8]) -> Result<StackItem> {
    let limits = ExecutionEngineLimits::default();
    BinarySerializer::deserialize(bytes, &limits, None).map_err(|err| anyhow!("{err}"))
}

pub(super) fn stack_item_bytes(item: &StackItem) -> Option<Vec<u8>> {
    match item {
        StackItem::ByteString(bytes) => Some(bytes.clone()),
        StackItem::Buffer(buffer) => Some(buffer.data()),
        _ => None,
    }
}

pub(super) fn stack_item_u32(item: &StackItem) -> Option<u32> {
    item.as_int().ok()?.to_u32()
}
