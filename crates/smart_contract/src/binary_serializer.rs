//! Binary serializer for Neo VM stack items.
//!
//! This is a faithful port of `Neo.SmartContract.BinarySerializer` from the
//! C# reference node. It provides serialization and deserialization utilities
//! for `StackItem` values while enforcing the execution engine limits and
//! reference tracking semantics used by the original implementation.

use crate::{Error, Result};
use neo_io::{BinaryWriter, MemoryReader};
use neo_vm::stack_item::array::Array;
use neo_vm::stack_item::map::Map;
use neo_vm::stack_item::primitive_type::PrimitiveTypeExt;
use neo_vm::stack_item::struct_item::Struct;
use neo_vm::{ExecutionEngineLimits, ReferenceCounter, StackItem, StackItemType};
use num_bigint::BigInt;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

const INTEGER_MAX_SIZE: usize = 32;

#[derive(Debug, Clone, Default)]
pub struct BinarySerializer;

impl BinarySerializer {
    /// Serializes the provided value into a byte buffer using the supplied limits.
    pub fn serialize<T: IntoStackItem>(value: T, limits: ExecutionEngineLimits) -> Result<Vec<u8>> {
        let item = value.into_stack_item();
        Self::serialize_with_limits(
            item,
            limits.max_item_size as usize,
            limits.max_stack_size as usize,
        )
    }

    /// Serializes a stack item using explicit size and item limits.
    pub fn serialize_with_limits(
        item: StackItem,
        max_size: usize,
        max_items: usize,
    ) -> Result<Vec<u8>> {
        let mut writer = BinaryWriter::new();
        Self::serialize_into_writer(&mut writer, item, max_size, max_items)?;
        Ok(writer.to_bytes())
    }

    /// Serializes a value into an existing writer using explicit limits.
    pub fn serialize_into_writer<T: IntoStackItem>(
        writer: &mut BinaryWriter,
        value: T,
        max_size: usize,
        max_items: usize,
    ) -> Result<()> {
        Self::serialize_internal(writer, value.into_stack_item(), max_size, max_items)
    }

    /// Deserializes a stack item from a byte slice.
    pub fn deserialize(
        data: &[u8],
        limits: ExecutionEngineLimits,
        reference_counter: Option<Arc<ReferenceCounter>>,
    ) -> Result<StackItem> {
        let mut reader = MemoryReader::new(data);
        let capped_size = std::cmp::min(data.len(), limits.max_item_size as usize) as u32;
        Self::deserialize_internal(
            &mut reader,
            capped_size,
            limits.max_stack_size,
            reference_counter,
        )
    }

    /// Deserializes a stack item using the supplied reader and execution limits.
    pub fn deserialize_from_reader(
        reader: &mut MemoryReader,
        limits: ExecutionEngineLimits,
        reference_counter: Option<Arc<ReferenceCounter>>,
    ) -> Result<StackItem> {
        Self::deserialize_internal(
            reader,
            limits.max_item_size,
            limits.max_stack_size,
            reference_counter,
        )
    }

    /// Deserializes a stack item using explicit size and item limits.
    pub fn deserialize_with_limits(
        reader: &mut MemoryReader,
        max_size: u32,
        max_items: u32,
        reference_counter: Option<Arc<ReferenceCounter>>,
    ) -> Result<StackItem> {
        Self::deserialize_internal(reader, max_size, max_items, reference_counter)
    }

    fn serialize_internal(
        writer: &mut BinaryWriter,
        item: StackItem,
        max_size: usize,
        max_items: usize,
    ) -> Result<()> {
        if max_items == 0 {
            return Err(format_error(
                "Maximum stack item count must be greater than zero",
            ));
        }

        let mut serialized = HashSet::<ContainerIdentity>::new();
        let mut stack = vec![item];
        let mut remaining = max_items;

        while let Some(current) = stack.pop() {
            if remaining == 0 {
                return Err(format_error(
                    "Serialized item count exceeds execution limits",
                ));
            }
            remaining -= 1;

            writer.write_u8(current.stack_item_type().to_byte())?;

            match current {
                StackItem::Null => {}
                StackItem::Boolean(value) => {
                    writer.write_bool(value)?;
                }
                other @ (StackItem::Integer(_)
                | StackItem::ByteString(_)
                | StackItem::Buffer(_)) => {
                    let bytes = other.as_bytes()?;
                    writer.write_var_bytes(&bytes)?;
                }
                StackItem::Array(array) => {
                    Self::track_container(&mut serialized, ContainerIdentity::from_array(&array))?;
                    writer.write_var_int(array.len() as u64)?;
                    for child in array.items().iter().rev() {
                        stack.push(child.clone());
                    }
                }
                StackItem::Struct(struct_item) => {
                    Self::track_container(
                        &mut serialized,
                        ContainerIdentity::from_struct(&struct_item),
                    )?;
                    writer.write_var_int(struct_item.len() as u64)?;
                    for child in struct_item.items().iter().rev() {
                        stack.push(child.clone());
                    }
                }
                StackItem::Map(map) => {
                    Self::track_container(&mut serialized, ContainerIdentity::from_map(&map))?;
                    writer.write_var_int(map.len() as u64)?;
                    for (key, value) in map.items().iter().rev() {
                        stack.push(value.clone());
                        stack.push(key.clone());
                    }
                }
                StackItem::Pointer(_) | StackItem::InteropInterface(_) => {
                    return Err(unsupported_error("Stack item type cannot be serialized"));
                }
            }

            if writer.len() > max_size {
                return Err(Error::InvalidOperation(
                    "Serialized data exceeds configured maximum size".to_string(),
                ));
            }
        }

        Ok(())
    }

    fn deserialize_internal(
        reader: &mut MemoryReader,
        max_size: u32,
        max_items: u32,
        reference_counter: Option<Arc<ReferenceCounter>>,
    ) -> Result<StackItem> {
        if max_items == 0 {
            return Err(format_error(
                "Maximum stack item count must be greater than zero",
            ));
        }

        let max_items_usize = max_items as usize;
        let mut entries: Vec<DeserializedEntry> = Vec::new();
        let mut pending: u64 = 1;

        while pending > 0 {
            pending -= 1;

            let raw_type = reader.read_byte()?;
            let item_type = StackItemType::from_byte(raw_type).ok_or_else(|| {
                format_error(format!("Unsupported stack item type 0x{raw_type:02X}"))
            })?;

            match item_type {
                StackItemType::Any => entries.push(DeserializedEntry::Item(StackItem::null())),
                StackItemType::Boolean => {
                    let value = reader.read_boolean()?;
                    entries.push(DeserializedEntry::Item(StackItem::from_bool(value)));
                }
                StackItemType::Integer => {
                    let bytes = reader.read_var_memory(INTEGER_MAX_SIZE)?;
                    let value = BigInt::from_signed_bytes_le(&bytes);
                    entries.push(DeserializedEntry::Item(StackItem::from_int(value)));
                }
                StackItemType::ByteString => {
                    let bytes = reader.read_var_memory(max_size as usize)?;
                    entries.push(DeserializedEntry::Item(StackItem::from_byte_string(bytes)));
                }
                StackItemType::Buffer => {
                    let bytes = reader.read_var_memory(max_size as usize)?;
                    entries.push(DeserializedEntry::Item(StackItem::from_buffer(bytes)));
                }
                StackItemType::Array | StackItemType::Struct => {
                    let count = reader.read_var_int(max_items as u64)? as usize;
                    Self::validate_container_size(count, max_items_usize)?;
                    pending = pending
                        .checked_add(count as u64)
                        .ok_or_else(|| format_error("Array size exceeds execution limits"))?;
                    entries.push(DeserializedEntry::Placeholder {
                        ty: item_type,
                        element_count: count,
                    });
                }
                StackItemType::Map => {
                    let count = reader.read_var_int(max_items as u64)? as usize;
                    Self::validate_container_size(count * 2, max_items_usize)?;
                    pending = pending
                        .checked_add((count as u64).checked_mul(2).ok_or_else(|| {
                            format_error("Map entry count exceeds execution limits")
                        })?)
                        .ok_or_else(|| format_error("Map entry count exceeds execution limits"))?;
                    entries.push(DeserializedEntry::Placeholder {
                        ty: item_type,
                        element_count: count,
                    });
                }
                _ => {
                    return Err(format_error(format!(
                        "Unsupported stack item type 0x{raw_type:02X}"
                    )));
                }
            }

            if entries.len() > max_items_usize {
                return Err(format_error(
                    "Deserialized item count exceeds maximum stack size",
                ));
            }
        }

        let mut stack: Vec<StackItem> = Vec::with_capacity(entries.len());

        while let Some(entry) = entries.pop() {
            match entry {
                DeserializedEntry::Item(item) => stack.push(item),
                DeserializedEntry::Placeholder { ty, element_count } => match ty {
                    StackItemType::Array => {
                        let mut array = Array::new(
                            Vec::with_capacity(element_count),
                            reference_counter.clone(),
                        );
                        for _ in 0..element_count {
                            let item = stack.pop().ok_or_else(|| {
                                format_error("Malformed array serialization payload")
                            })?;
                            array.push(item);
                        }
                        stack.push(StackItem::Array(array));
                    }
                    StackItemType::Struct => {
                        let mut structure = Struct::new(
                            Vec::with_capacity(element_count),
                            reference_counter.clone(),
                        );
                        for _ in 0..element_count {
                            let item = stack.pop().ok_or_else(|| {
                                format_error("Malformed struct serialization payload")
                            })?;
                            structure.push(item);
                        }
                        stack.push(StackItem::Struct(structure));
                    }
                    StackItemType::Map => {
                        let mut map = Map::new(BTreeMap::new(), reference_counter.clone());
                        for _ in 0..element_count {
                            let key = stack
                                .pop()
                                .ok_or_else(|| format_error("Malformed map key payload"))?;
                            key.as_primitive()?;
                            let value = stack
                                .pop()
                                .ok_or_else(|| format_error("Malformed map value payload"))?;
                            map.set(key, value)?;
                        }
                        stack.push(StackItem::Map(map));
                    }
                    _ => return Err(format_error("Unexpected container placeholder type")),
                },
            }
        }

        stack
            .pop()
            .ok_or_else(|| format_error("Empty serialization payload"))
    }

    fn validate_container_size(count: usize, max_items: usize) -> Result<()> {
        if count > max_items {
            return Err(format_error(
                "Container entry count exceeds maximum stack size",
            ));
        }
        Ok(())
    }

    fn track_container(
        seen: &mut HashSet<ContainerIdentity>,
        identity: ContainerIdentity,
    ) -> Result<()> {
        if !seen.insert(identity) {
            return Err(unsupported_error(
                "Circular reference detected during serialization",
            ));
        }
        Ok(())
    }
}

#[derive(Debug)]
enum DeserializedEntry {
    Item(StackItem),
    Placeholder {
        ty: StackItemType,
        element_count: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ContainerIdentity {
    Reference(usize),
    Pointer(usize),
}

impl ContainerIdentity {
    fn from_array(array: &Array) -> Self {
        array
            .reference_id()
            .map(ContainerIdentity::Reference)
            .unwrap_or_else(|| ContainerIdentity::Pointer(array as *const Array as usize))
    }

    fn from_struct(struct_item: &Struct) -> Self {
        struct_item
            .reference_id()
            .map(ContainerIdentity::Reference)
            .unwrap_or_else(|| ContainerIdentity::Pointer(struct_item as *const Struct as usize))
    }

    fn from_map(map: &Map) -> Self {
        map.reference_id()
            .map(ContainerIdentity::Reference)
            .unwrap_or_else(|| ContainerIdentity::Pointer(map as *const Map as usize))
    }
}

fn format_error(message: impl Into<String>) -> Error {
    Error::SerializationError(message.into())
}

fn unsupported_error(message: impl Into<String>) -> Error {
    Error::InvalidOperation(message.into())
}

/// Helper trait that mirrors the implicit conversions provided by the C#
/// implementation. This allows the serializer to accept primitive rust values
/// in addition to pre-built stack items.
pub trait IntoStackItem {
    fn into_stack_item(self) -> StackItem;
}

impl IntoStackItem for StackItem {
    fn into_stack_item(self) -> StackItem {
        self
    }
}

impl<'a> IntoStackItem for &'a StackItem {
    fn into_stack_item(self) -> StackItem {
        self.clone()
    }
}

impl IntoStackItem for bool {
    fn into_stack_item(self) -> StackItem {
        StackItem::from_bool(self)
    }
}

impl IntoStackItem for i32 {
    fn into_stack_item(self) -> StackItem {
        StackItem::from_int(self)
    }
}

impl IntoStackItem for i64 {
    fn into_stack_item(self) -> StackItem {
        StackItem::from_int(self)
    }
}

impl IntoStackItem for u32 {
    fn into_stack_item(self) -> StackItem {
        StackItem::from_int(self)
    }
}

impl IntoStackItem for u64 {
    fn into_stack_item(self) -> StackItem {
        StackItem::from_int(self)
    }
}

impl IntoStackItem for Vec<u8> {
    fn into_stack_item(self) -> StackItem {
        StackItem::from_byte_string(self)
    }
}

impl<'a> IntoStackItem for &'a [u8] {
    fn into_stack_item(self) -> StackItem {
        StackItem::from_byte_string(self.to_vec())
    }
}

impl<'a> IntoStackItem for &'a Vec<u8> {
    fn into_stack_item(self) -> StackItem {
        StackItem::from_byte_string(self.clone())
    }
}
