#![allow(clippy::mutable_key_type)]

//! BinarySerializer - aligns with `Neo.SmartContract.BinarySerializer`.

use crate::neo_io::{IoError, MemoryReader};
use neo_vm::execution_engine_limits::ExecutionEngineLimits;
use neo_vm::reference_counter::ReferenceCounter;
use neo_vm::stack_item::array::Array as ArrayItem;
use neo_vm::stack_item::buffer::Buffer as BufferItem;
use neo_vm::stack_item::map::Map as MapItem;
use neo_vm::stack_item::struct_item::Struct as StructItem;
use neo_vm::{StackItem, StackItemType};
use num_bigint::BigInt;
use std::collections::{BTreeMap, HashSet, VecDeque};

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
        reference_counter: Option<ReferenceCounter>,
    ) -> Result<StackItem, String> {
        let mut reader = MemoryReader::new(data);
        Self::deserialize_with_limits(
            &mut reader,
            limits.max_item_size,
            limits.max_stack_size,
            reference_counter,
        )
    }

    /// Deserialize using explicit limits (mirrors the C# overload).
    pub fn deserialize_with_limits(
        reader: &mut MemoryReader<'_>,
        max_size: u32,
        max_items: u32,
        reference_counter: Option<ReferenceCounter>,
    ) -> Result<StackItem, String> {
        let mut pending: Vec<PendingItem> = Vec::new();
        let mut remaining = 1usize;
        let mut total_items = 0usize;

        while remaining > 0 {
            remaining -= 1;
            if total_items >= max_items as usize {
                return Err("Too many items".to_string());
            }

            let item_type_byte = reader.read_byte().map_err(Self::io_error_to_string)?;
            let item_type = StackItemType::from_u8(item_type_byte)
                .ok_or_else(|| format!("Unknown stack item type: 0x{item_type_byte:02x}"))?;

            match item_type {
                StackItemType::Any => {
                    pending.push(PendingItem::Value(StackItem::null()));
                    total_items += 1;
                }
                StackItemType::Boolean => {
                    let value = reader.read_boolean().map_err(Self::io_error_to_string)?;
                    pending.push(PendingItem::Value(StackItem::from_bool(value)));
                    total_items += 1;
                }
                StackItemType::Integer => {
                    let bytes = reader
                        .read_var_bytes(Self::MAX_INTEGER_SIZE)
                        .map_err(Self::io_error_to_string)?;
                    let value = if bytes.is_empty() {
                        BigInt::from(0)
                    } else {
                        BigInt::from_signed_bytes_le(&bytes)
                    };
                    pending.push(PendingItem::Value(StackItem::from_int(value)));
                    total_items += 1;
                }
                StackItemType::ByteString => {
                    let bytes = reader
                        .read_var_bytes(max_size as usize)
                        .map_err(Self::io_error_to_string)?;
                    pending.push(PendingItem::Value(StackItem::from_byte_string(bytes)));
                    total_items += 1;
                }
                StackItemType::Buffer => {
                    let bytes = reader
                        .read_var_bytes(max_size as usize)
                        .map_err(Self::io_error_to_string)?;
                    let buffer = BufferItem::new(bytes.to_vec());
                    pending.push(PendingItem::Value(StackItem::Buffer(buffer)));
                    total_items += 1;
                }
                StackItemType::Array | StackItemType::Struct => {
                    let count = reader
                        .read_var_int(max_items as u64)
                        .map_err(Self::io_error_to_string)?
                        as usize;
                    if count > (max_items as usize).saturating_sub(total_items) {
                        return Err("Too many items".to_string());
                    }
                    pending.push(PendingItem::Container(ContainerDescriptor {
                        item_type,
                        element_count: count,
                    }));
                    remaining += count;
                    total_items += count + 1;
                }
                StackItemType::Map => {
                    let count = reader
                        .read_var_int(max_items as u64)
                        .map_err(Self::io_error_to_string)?
                        as usize;
                    if count > (max_items as usize).saturating_sub(total_items) {
                        return Err("Too many items".to_string());
                    }
                    pending.push(PendingItem::Container(ContainerDescriptor {
                        item_type,
                        element_count: count,
                    }));
                    remaining += count * 2;
                    total_items += count * 2 + 1;
                }
                _ => return Err("Unsupported stack item type".to_string()),
            }

            if pending.len() > max_items as usize {
                return Err("Too many items".to_string());
            }
        }

        let mut constructed: Vec<StackItem> = Vec::new();
        while let Some(item) = pending.pop() {
            match item {
                PendingItem::Value(stack_item) => constructed.push(stack_item),
                PendingItem::Container(container) => {
                    let rc = reference_counter.clone();
                    let result =
                        match container.item_type {
                            StackItemType::Array => {
                                let mut elements = Vec::with_capacity(container.element_count);
                                for _ in 0..container.element_count {
                                    elements.push(constructed.pop().ok_or_else(|| {
                                        "Invalid serialized array data".to_string()
                                    })?);
                                }
                                StackItem::Array(
                                    ArrayItem::new(elements, rc.clone())
                                        .map_err(|err| err.to_string())?,
                                )
                            }
                            StackItemType::Struct => {
                                let mut elements = Vec::with_capacity(container.element_count);
                                for _ in 0..container.element_count {
                                    elements.push(constructed.pop().ok_or_else(|| {
                                        "Invalid serialized struct data".to_string()
                                    })?);
                                }
                                StackItem::Struct(
                                    StructItem::new(elements, rc.clone())
                                        .map_err(|err| err.to_string())?,
                                )
                            }
                            StackItemType::Map => {
                                let mut entries = BTreeMap::new();
                                for _ in 0..container.element_count {
                                    let key = constructed
                                        .pop()
                                        .ok_or_else(|| "Invalid serialized map key".to_string())?;
                                    let value = constructed.pop().ok_or_else(|| {
                                        "Invalid serialized map value".to_string()
                                    })?;
                                    entries.insert(key, value);
                                }
                                StackItem::Map(
                                    MapItem::new(entries, rc.clone())
                                        .map_err(|err| err.to_string())?,
                                )
                            }
                            _ => return Err("Invalid container descriptor".to_string()),
                        };
                    constructed.push(result);
                }
            }
        }

        constructed
            .pop()
            .ok_or_else(|| "Empty serialization payload".to_string())
    }

    /// Serialize a stack item with VM limits.
    pub fn serialize(item: &StackItem, limits: &ExecutionEngineLimits) -> Result<Vec<u8>, String> {
        Self::serialize_with_limits(
            item,
            limits.max_item_size as usize,
            limits.max_stack_size as usize,
        )
    }

    /// Serialize a stack item using explicit limits.
    pub fn serialize_with_limits(
        item: &StackItem,
        max_size: usize,
        max_items: usize,
    ) -> Result<Vec<u8>, String> {
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
    ) -> Result<(), String> {
        let mut processed: HashSet<(usize, StackItemType)> = HashSet::new();
        let mut queue: VecDeque<StackItem> = VecDeque::new();
        queue.push_back(item.clone());
        let mut remaining = max_items as isize;

        while let Some(current) = queue.pop_back() {
            remaining -= 1;
            if remaining < 0 {
                return Err("Too many items".to_string());
            }

            match &current {
                StackItem::Null => {
                    writer.push(StackItemType::Any as u8);
                }
                StackItem::Boolean(value) => {
                    writer.push(StackItemType::Boolean as u8);
                    writer.push(if *value { 1 } else { 0 });
                }
                StackItem::Integer(integer) => {
                    writer.push(StackItemType::Integer as u8);
                    let (sign, bytes) = integer.to_bytes_le();
                    if sign == num_bigint::Sign::Minus {
                        return Err("Negative integers are not supported".to_string());
                    }
                    Self::write_var_bytes(writer, &bytes)?;
                }
                StackItem::ByteString(bytes) => {
                    writer.push(StackItemType::ByteString as u8);
                    Self::write_var_bytes(writer, bytes)?;
                }
                StackItem::Buffer(buffer) => {
                    writer.push(StackItemType::Buffer as u8);
                    let data = buffer.data();
                    Self::write_var_bytes(writer, &data)?;
                }
                StackItem::Array(array) => {
                    writer.push(StackItemType::Array as u8);
                    let identity = (array.id(), StackItemType::Array);
                    if !processed.insert(identity) {
                        return Err(
                            "Circular reference detected while serializing array".to_string()
                        );
                    }
                    Self::write_var_int(writer, array.len())?;
                    for element in array.items().iter().rev() {
                        queue.push_back(element.clone());
                    }
                }
                StackItem::Struct(struct_item) => {
                    writer.push(StackItemType::Struct as u8);
                    let identity = (struct_item.id(), StackItemType::Struct);
                    if !processed.insert(identity) {
                        return Err(
                            "Circular reference detected while serializing struct".to_string()
                        );
                    }
                    Self::write_var_int(writer, struct_item.len())?;
                    for element in struct_item.items().iter().rev() {
                        queue.push_back(element.clone());
                    }
                }
                StackItem::Map(map) => {
                    writer.push(StackItemType::Map as u8);
                    let identity = (map.id(), StackItemType::Map);
                    if !processed.insert(identity) {
                        return Err("Circular reference detected while serializing map".to_string());
                    }
                    Self::write_var_int(writer, map.len())?;
                    for (key, value) in map.items().iter().rev() {
                        queue.push_back(value.clone());
                        queue.push_back(key.clone());
                    }
                }
                _ => return Err("Unsupported stack item type".to_string()),
            }

            if writer.len() > max_size {
                return Err("Serialized data exceeds limit".to_string());
            }
        }

        Ok(())
    }

    fn write_var_int(writer: &mut Vec<u8>, value: usize) -> Result<(), String> {
        if value < 0xfd {
            writer.push(value as u8);
        } else if value <= 0xffff {
            writer.push(0xfd);
            writer.extend_from_slice(&(value as u16).to_le_bytes());
        } else if value <= 0xffff_ffff {
            writer.push(0xfe);
            writer.extend_from_slice(&(value as u32).to_le_bytes());
        } else {
            writer.push(0xff);
            writer.extend_from_slice(&(value as u64).to_le_bytes());
        }
        Ok(())
    }

    fn write_var_bytes(writer: &mut Vec<u8>, bytes: &[u8]) -> Result<(), String> {
        Self::write_var_int(writer, bytes.len())?;
        writer.extend_from_slice(bytes);
        Ok(())
    }

    fn io_error_to_string(error: IoError) -> String {
        match error {
            IoError::Format => "format error".to_string(),
            IoError::InvalidUtf8 => "invalid utf-8 data".to_string(),
            IoError::InvalidData { context, value } => format!("{context}: {value}"),
            IoError::Io(inner) => inner.to_string(),
        }
    }
}

trait StackItemTypeExt {
    fn from_u8(value: u8) -> Option<StackItemType>;
}

impl StackItemTypeExt for StackItemType {
    fn from_u8(value: u8) -> Option<StackItemType> {
        match value {
            0x00 => Some(StackItemType::Any),
            0x10 => Some(StackItemType::Pointer),
            0x20 => Some(StackItemType::Boolean),
            0x21 => Some(StackItemType::Integer),
            0x28 => Some(StackItemType::ByteString),
            0x30 => Some(StackItemType::Buffer),
            0x40 => Some(StackItemType::Array),
            0x41 => Some(StackItemType::Struct),
            0x48 => Some(StackItemType::Map),
            0x60 => Some(StackItemType::InteropInterface),
            _ => None,
        }
    }
}
