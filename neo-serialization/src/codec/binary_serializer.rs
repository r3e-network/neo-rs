#![allow(clippy::mutable_key_type)]

//! BinarySerializer - aligns with `Neo.SmartContract.BinarySerializer`.

use neo_error::{CoreError, CoreResult};
use neo_io::var_int::VarInt;
use neo_io::{IoError, MemoryReader};
use neo_vm::ExecutionEngineLimits;
use neo_vm::StackItem;
use neo_vm::StackItemType;
use neo_vm::reference_counter::ReferenceCounter;
use num_bigint::BigInt;
use std::collections::{HashSet, VecDeque};
/// Binary serializer helpers for VM stack items.
pub struct BinarySerializer;

#[derive(Debug, Clone, Copy)]
struct ContainerDescriptor {
    item_type: StackItemType,
    element_count: usize,
}

#[derive(Debug)]
enum PendingItem {
    Value(StackItem),
    Container(ContainerDescriptor),
}

impl BinarySerializer {
    const MAX_INTEGER_SIZE: usize = 32;

    /// Deserialize a [`StackItem`] from the provided buffer using VM limits.
    pub fn deserialize(
        data: &[u8],
        limits: &ExecutionEngineLimits,
        _reference_counter: Option<ReferenceCounter>,
    ) -> CoreResult<StackItem> {
        let mut reader = MemoryReader::new(data);
        Self::deserialize_with_limits(
            &mut reader,
            limits.max_item_size,
            limits.max_stack_size,
            _reference_counter,
        )
    }

    /// Deserialize a [`StackItem`] from the provided buffer using default VM limits.
    pub fn deserialize_default(data: &[u8]) -> CoreResult<StackItem> {
        Self::deserialize(data, &ExecutionEngineLimits::default(), None)
    }

    /// Deserialize using explicit limits (mirrors the C# overload).
    pub fn deserialize_with_limits(
        reader: &mut MemoryReader<'_>,
        max_size: u32,
        max_items: u32,
        _reference_counter: Option<ReferenceCounter>,
    ) -> CoreResult<StackItem> {
        let mut pending: Vec<PendingItem> = Vec::new();
        let mut remaining = 1usize;
        while remaining > 0 {
            remaining -= 1;

            let item_type_byte = reader.read_byte().map_err(Self::io_error_to_core_error)?;
            let item_type = StackItemType::from_byte(item_type_byte).ok_or_else(|| {
                CoreError::other(format!("Unknown stack item type: 0x{item_type_byte:02x}"))
            })?;

            match item_type {
                StackItemType::Any => {
                    pending.push(PendingItem::Value(StackItem::null()));
                }
                StackItemType::Boolean => {
                    let value = reader
                        .read_boolean()
                        .map_err(Self::io_error_to_core_error)?;
                    pending.push(PendingItem::Value(StackItem::from_bool(value)));
                }
                StackItemType::Integer => {
                    let bytes = reader
                        .read_var_bytes(Self::MAX_INTEGER_SIZE)
                        .map_err(Self::io_error_to_core_error)?;
                    let value = if bytes.is_empty() {
                        BigInt::from(0)
                    } else {
                        BigInt::from_signed_bytes_le(&bytes)
                    };
                    pending.push(PendingItem::Value(StackItem::from_int(value)));
                }
                StackItemType::ByteString => {
                    let bytes = reader
                        .read_var_bytes(max_size as usize)
                        .map_err(Self::io_error_to_core_error)?;
                    pending.push(PendingItem::Value(StackItem::from_byte_string(bytes)));
                }
                StackItemType::Buffer => {
                    let bytes = reader
                        .read_var_bytes(max_size as usize)
                        .map_err(Self::io_error_to_core_error)?;
                    pending.push(PendingItem::Value(StackItem::from_buffer(bytes)));
                }
                StackItemType::Array | StackItemType::Struct => {
                    let count = reader
                        .read_var_int(max_items as u64)
                        .map_err(Self::io_error_to_core_error)?
                        as usize;
                    pending.push(PendingItem::Container(ContainerDescriptor {
                        item_type,
                        element_count: count,
                    }));
                    remaining = remaining
                        .checked_add(count)
                        .ok_or_else(|| CoreError::other("Too many items"))?;
                }
                StackItemType::Map => {
                    let count = reader
                        .read_var_int(max_items as u64)
                        .map_err(Self::io_error_to_core_error)?
                        as usize;
                    let child_count = count
                        .checked_mul(2)
                        .ok_or_else(|| CoreError::other("Too many items"))?;
                    pending.push(PendingItem::Container(ContainerDescriptor {
                        item_type,
                        element_count: count,
                    }));
                    remaining = remaining
                        .checked_add(child_count)
                        .ok_or_else(|| CoreError::other("Too many items"))?;
                }
                _ => return Err(CoreError::other("Unsupported stack item type")),
            }

            if pending.len() > max_items as usize {
                return Err(CoreError::other("Too many items"));
            }
        }

        let mut constructed: Vec<StackItem> = Vec::new();
        while let Some(item) = pending.pop() {
            match item {
                PendingItem::Value(stack_item) => constructed.push(stack_item),
                PendingItem::Container(container) => {
                    let result = match container.item_type {
                        StackItemType::Array => {
                            let mut elements = Vec::with_capacity(container.element_count);
                            for _ in 0..container.element_count {
                                elements.push(constructed.pop().ok_or_else(|| {
                                    CoreError::other("Invalid serialized array data")
                                })?);
                            }
                            StackItem::from_array(elements)
                        }
                        StackItemType::Struct => {
                            let mut elements = Vec::with_capacity(container.element_count);
                            for _ in 0..container.element_count {
                                elements.push(constructed.pop().ok_or_else(|| {
                                    CoreError::other("Invalid serialized struct data")
                                })?);
                            }
                            StackItem::from_struct(elements)
                        }
                        StackItemType::Map => {
                            let mut entries = Vec::with_capacity(container.element_count);
                            for _ in 0..container.element_count {
                                let key = constructed.pop().ok_or_else(|| {
                                    CoreError::other("Invalid serialized map key")
                                })?;
                                let value = constructed.pop().ok_or_else(|| {
                                    CoreError::other("Invalid serialized map value")
                                })?;
                                entries.push((key, value));
                            }
                            let map = neo_vm::stack_item::Map::new(
                                neo_vm::VmOrderedDictionary::new(),
                                None,
                            )
                            .map_err(|error| CoreError::other(error.to_string()))?;
                            for (key, value) in entries {
                                map.set(key, value)
                                    .map_err(|error| CoreError::other(error.to_string()))?;
                            }
                            StackItem::Map(map)
                        }
                        _ => return Err(CoreError::other("Invalid container descriptor")),
                    };
                    constructed.push(result);
                }
            }
        }

        constructed
            .pop()
            .ok_or_else(|| CoreError::other("Empty serialization payload"))
    }

    /// Serialize a stack item with VM limits.
    pub fn serialize(item: &StackItem, limits: &ExecutionEngineLimits) -> CoreResult<Vec<u8>> {
        Self::serialize_with_limits(
            item,
            limits.max_item_size as usize,
            limits.max_stack_size as usize,
        )
    }

    /// Serialize a stack item with default VM limits.
    pub fn serialize_default(item: &StackItem) -> CoreResult<Vec<u8>> {
        Self::serialize(item, &ExecutionEngineLimits::default())
    }

    /// Serialize a stack item using explicit limits.
    pub fn serialize_with_limits(
        item: &StackItem,
        max_size: usize,
        max_items: usize,
    ) -> CoreResult<Vec<u8>> {
        let mut writer = Vec::new();
        Self::serialize_into(item, &mut writer, max_size, max_items)?;
        Ok(writer)
    }

    /// Serialize into the provided writer buffer.
    pub fn serialize_into(
        item: &StackItem,
        writer: &mut Vec<u8>,
        max_size: usize,
        max_items: usize,
    ) -> CoreResult<()> {
        let mut processed: HashSet<(usize, StackItemType)> = HashSet::new();
        let mut queue: VecDeque<StackItem> = VecDeque::new();
        queue.push_back(item.clone());
        let mut remaining = max_items as isize;

        while let Some(current) = queue.pop_back() {
            remaining -= 1;
            if remaining < 0 {
                return Err(CoreError::other("Too many items"));
            }

            match &current {
                StackItem::Null => {
                    writer.push(StackItemType::Any.to_byte());
                }
                StackItem::Boolean(value) => {
                    writer.push(StackItemType::Boolean.to_byte());
                    writer.push(if *value { 1 } else { 0 });
                }
                StackItem::Integer(integer) => {
                    writer.push(StackItemType::Integer.to_byte());
                    let bytes = if integer.is_zero() {
                        Vec::new()
                    } else {
                        integer.to_signed_bytes_le()
                    };
                    VarInt::write_var_bytes(&bytes, writer);
                }
                StackItem::ByteString(bytes) => {
                    writer.push(StackItemType::ByteString.to_byte());
                    VarInt::write_var_bytes(bytes, writer);
                }
                StackItem::Buffer(buffer) => {
                    writer.push(StackItemType::Buffer.to_byte());
                    VarInt::write_var_bytes(&buffer.data(), writer);
                }
                StackItem::Array(array) => {
                    writer.push(StackItemType::Array.to_byte());
                    let identity = (array.id(), StackItemType::Array);
                    if !processed.insert(identity) {
                        return Err(CoreError::other(
                            "Circular reference detected while serializing array",
                        ));
                    }
                    VarInt::write_var_int(array.len() as u64, writer);
                    for element in array.iter().rev() {
                        queue.push_back(element.clone());
                    }
                }
                StackItem::Struct(struct_item) => {
                    writer.push(StackItemType::Struct.to_byte());
                    let identity = (struct_item.id(), StackItemType::Struct);
                    if !processed.insert(identity) {
                        return Err(CoreError::other(
                            "Circular reference detected while serializing struct",
                        ));
                    }
                    VarInt::write_var_int(struct_item.len() as u64, writer);
                    for element in struct_item.iter().rev() {
                        queue.push_back(element.clone());
                    }
                }
                StackItem::Map(map) => {
                    writer.push(StackItemType::Map.to_byte());
                    let identity = (map.id(), StackItemType::Map);
                    if !processed.insert(identity) {
                        return Err(CoreError::other(
                            "Circular reference detected while serializing map",
                        ));
                    }
                    VarInt::write_var_int(map.len() as u64, writer);
                    for (key, value) in map.iter().rev() {
                        queue.push_back(value.clone());
                        queue.push_back(key.clone());
                    }
                }
                _ => return Err(CoreError::other("Unsupported stack item type")),
            }

            if writer.len() > max_size {
                return Err(CoreError::other("Serialized data exceeds limit"));
            }
        }

        Ok(())
    }

    fn io_error_to_core_error(error: IoError) -> CoreError {
        CoreError::other(match error {
            IoError::Format => "format error".to_string(),
            IoError::InvalidUtf8 => "invalid utf-8 data".to_string(),
            IoError::InvalidData { context, value } => format!("{context}: {value}"),
            IoError::Io(inner) => inner.to_string(),
        })
    }
}

#[cfg(test)]
#[path = "../tests/codec/binary_serializer.rs"]
mod tests;
