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
use num_traits::Zero;
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
                                let mut entries = Vec::with_capacity(container.element_count);
                                for _ in 0..container.element_count {
                                    let key = constructed
                                        .pop()
                                        .ok_or_else(|| "Invalid serialized map key".to_string())?;
                                    let value = constructed.pop().ok_or_else(|| {
                                        "Invalid serialized map value".to_string()
                                    })?;
                                    entries.push((key, value));
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
                    let bytes = if integer.is_zero() {
                        Vec::new()
                    } else {
                        integer.to_signed_bytes_le()
                    };
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

#[cfg(test)]
mod tests {
    use super::*;
    use neo_vm::execution_engine_limits::ExecutionEngineLimits;

    #[test]
    fn deserialize_preserves_map_entry_order_for_roundtrip_bytes() {
        let limits = ExecutionEngineLimits::default();

        // Serialize a map with specific insertion order: (3,30), (1,10), (2,20)
        let mut map_items = neo_vm::collections::VmOrderedDictionary::new();
        map_items.insert(StackItem::Integer(3.into()), StackItem::Integer(30.into()));
        map_items.insert(StackItem::Integer(1.into()), StackItem::Integer(10.into()));
        map_items.insert(StackItem::Integer(2.into()), StackItem::Integer(20.into()));

        let map = StackItem::Map(MapItem::new(map_items, None).unwrap());
        let serialized = BinarySerializer::serialize(&map, &limits).unwrap();

        // Deserialize and verify order is preserved
        let deserialized = BinarySerializer::deserialize(&serialized, &limits, None).unwrap();

        if let StackItem::Map(result_map) = deserialized {
            let items = result_map.items();
            assert_eq!(items.len(), 3);

            // Verify insertion order: (3,30), (1,10), (2,20)
            let items_vec: Vec<_> = items.iter().collect();
            assert_eq!(items_vec[0].0, &StackItem::Integer(3.into()));
            assert_eq!(items_vec[0].1, &StackItem::Integer(30.into()));
            assert_eq!(items_vec[1].0, &StackItem::Integer(1.into()));
            assert_eq!(items_vec[1].1, &StackItem::Integer(10.into()));
            assert_eq!(items_vec[2].0, &StackItem::Integer(2.into()));
            assert_eq!(items_vec[2].1, &StackItem::Integer(20.into()));
        } else {
            panic!("Expected Map");
        }
    }

    #[test]
    fn serialize_zero_integer_uses_empty_payload() {
        let limits = ExecutionEngineLimits::default();
        let serialized = BinarySerializer::serialize(&StackItem::from_int(0), &limits).unwrap();
        assert_eq!(serialized, vec![StackItemType::Integer as u8, 0]);
    }

    #[test]
    fn nep17_data_array_roundtrips_losslessly() {
        // Reproduces the `data` payload pushed by tx
        // 0x4e2d76756fe4253ed19ae68a99b3557b2dedfa3e8e204fddf61163c9334a7e17
        // (mainnet block 676,050) where N3Trader's onNEP17Payment diverges.
        // data = [Integer(0), [BS(""), BS("")], [GAS, GAS], [Int(100M), Int(100M)],
        //        [Integer(0), Integer(1)], Integer(-1)]
        let limits = ExecutionEngineLimits::default();
        let gas_hash =
            hex::decode("cf76e28bd0062c4a478ee35561011319f3cfa4d2").expect("gas hash");

        let inner_hashes = StackItem::from_array(vec![
            StackItem::from_byte_string(gas_hash.clone()),
            StackItem::from_byte_string(gas_hash.clone()),
        ]);
        let inner_amounts = StackItem::from_array(vec![
            StackItem::from_int(100_000_000i64),
            StackItem::from_int(100_000_000i64),
        ]);
        let inner_empty = StackItem::from_array(vec![
            StackItem::from_byte_string(Vec::new()),
            StackItem::from_byte_string(Vec::new()),
        ]);
        let inner_zero_one = StackItem::from_array(vec![
            StackItem::from_int(0i64),
            StackItem::from_int(1i64),
        ]);

        let data = StackItem::from_array(vec![
            StackItem::from_int(0i64),
            inner_empty.clone(),
            inner_hashes.clone(),
            inner_amounts.clone(),
            inner_zero_one.clone(),
            StackItem::from_int(-1i64),
        ]);

        let serialized = BinarySerializer::serialize(&data, &limits).expect("serialize");
        let deserialized =
            BinarySerializer::deserialize(&serialized, &limits, None).expect("deserialize");

        assert_eq!(
            deserialized.stack_item_type(),
            StackItemType::Array,
            "Top-level type must roundtrip as Array"
        );
        let arr = match &deserialized {
            StackItem::Array(a) => a.clone(),
            _ => panic!("Expected Array"),
        };
        assert_eq!(arr.len(), 6, "Array must have 6 elements");
        assert_eq!(arr.get(0).unwrap(), StackItem::from_int(0i64));
        assert_eq!(arr.get(5).unwrap(), StackItem::from_int(-1i64));

        // Re-serialize the deserialized form and confirm bytes match.
        let reserialized =
            BinarySerializer::serialize(&deserialized, &limits).expect("reserialize");
        assert_eq!(
            serialized, reserialized,
            "Roundtrip must produce identical bytes"
        );
    }
}
