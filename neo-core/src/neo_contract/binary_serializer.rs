use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Write};
use num_bigint::BigInt;
use neo_vm::vm::ExecutionEngineLimits;
use neo_vm::{References, StackItemType};
use neo_vm::StackItem;
use crate::io::memory_reader::MemoryReader;

/// A binary serializer for `StackItem`.
pub struct BinarySerializer;

struct ContainerPlaceholder {
    item_type: StackItemType,
    element_count: usize,
}

impl BinarySerializer {
    /// Deserializes a `StackItem` from byte array.
    pub fn deserialize(data: &[u8], limits: &ExecutionEngineLimits, reference_counter: Option<&References>) -> Result<StackItem, String> {
        let mut reader = MemoryReader::new(data);
        Self::deserialize_from_reader(&mut reader, limits.max_item_size.min(data.len()), limits.max_stack_size as usize, reference_counter)
    }

    /// Deserializes a `StackItem` from `MemoryReader`.
    pub fn deserialize_from_reader(reader: &mut MemoryReader, max_size: usize, max_items: usize, reference_counter: Option<&References>) -> Result<StackItem, String> {
        let mut deserialized = Vec::new();
        let mut undeserialized = 1;
        while undeserialized > 0 {
            undeserialized -= 1;
            let item_type = StackItemType::try_from(reader.read_u8()?).map_err(|e| e.to_string())?;
            match item_type {
                StackItemType::Any => deserialized.push(StackItem::Null),
                StackItemType::Boolean => deserialized.push(StackItem::Boolean(reader.read_bool()?)),
                StackItemType::Integer => {
                    let bytes = reader.read_var_bytes(Integer::MAX_SIZE)?;
                    deserialized.push(StackItem::Integer(BigInt::from_signed_bytes_be(&bytes)));
                },
                StackItemType::ByteString => deserialized.push(StackItem::ByteString(reader.read_var_bytes(max_size)?)),
                StackItemType::Buffer => {
                    let bytes = reader.read_var_bytes(max_size)?;
                    deserialized.push(StackItem::Buffer(bytes));
                },
                StackItemType::Array | StackItemType::Struct => {
                    let count = reader.read_var_int(max_items as u64)? as usize;
                    deserialized.push(ContainerPlaceholder { item_type, element_count: count });
                    undeserialized += count;
                },
                StackItemType::Map => {
                    let count = reader.read_var_int(max_items as u64)? as usize;
                    deserialized.push(ContainerPlaceholder { item_type, element_count: count });
                    undeserialized += count * 2;
                },
                _ => return Err("Invalid StackItemType".to_string()),
            }
            if deserialized.len() > max_items {
                return Err("Exceeded maximum number of items".to_string());
            }
        }

        let mut stack_temp = Vec::new();
        while let Some(item) = deserialized.pop() {
            match item {
                StackItem::Any | StackItem::Boolean(_) | StackItem::Integer(_) | StackItem::ByteString(_) | StackItem::Buffer(_) => {
                    stack_temp.push(item);
                },
                ContainerPlaceholder { item_type, element_count } => {
                    match item_type {
                        StackItemType::Array => {
                            let mut array = Array::new(reference_counter);
                            for _ in 0..element_count {
                                array.add(stack_temp.pop().unwrap());
                            }
                            stack_temp.push(StackItem::Array(array));
                        },
                        StackItemType::Struct => {
                            let mut struct_item = Struct::new(reference_counter);
                            for _ in 0..element_count {
                                struct_item.add(stack_temp.pop().unwrap());
                            }
                            stack_temp.push(StackItem::Struct(struct_item));
                        },
                        StackItemType::Map => {
                            let mut map = Map::new(reference_counter);
                            for _ in 0..element_count {
                                let key = stack_temp.pop().unwrap();
                                let value = stack_temp.pop().unwrap();
                                map.insert(key.into_primitive().unwrap(), value);
                            }
                            stack_temp.push(StackItem::Map(map));
                        },
                        _ => unreachable!(),
                    }
                },
                _ => unreachable!(),
            }
        }

        Ok(stack_temp.pop().unwrap())
    }

    /// Serializes a `StackItem` to byte array.
    pub fn serialize(item: &StackItem, limits: &ExecutionEngineLimits) -> Result<Vec<u8>, String> {
        Self::serialize_with_limits(item, limits.max_item_size as usize, limits.max_stack_size as usize)
    }

    /// Serializes a `StackItem` to byte array with custom limits.
    pub fn serialize_with_limits(item: &StackItem, max_size: usize, max_items: usize) -> Result<Vec<u8>, String> {
        let mut writer = Cursor::new(Vec::new());
        Self::serialize_to_writer(&mut writer, item, max_size, max_items)?;
        Ok(writer.into_inner())
    }

    /// Serializes a `StackItem` into `BinaryWriter`.
    pub fn serialize_to_writer<W: Write>(writer: &mut W, item: &StackItem, max_size: usize, max_items: usize) -> Result<(), String> {
        let mut serialized = HashSet::new();
        let mut unserialized = vec![item];
        let mut items_count = 0;

        while let Some(item) = unserialized.pop() {
            if items_count >= max_items {
                return Err("Exceeded maximum number of items".to_string());
            }
            items_count += 1;

            writer.write_all(&[item.type_() as u8]).map_err(|e| e.to_string())?;

            match item {
                StackItem::Any => {},
                StackItem::Boolean(b) => writer.write_all(&[*b as u8]).map_err(|e| e.to_string())?,
                StackItem::Integer(i) => {
                    let bytes = i.to_signed_bytes_be();
                    writer.write_var_bytes(&bytes).map_err(|e| e.to_string())?;
                },
                StackItem::ByteString(bs) | StackItem::Buffer(bs) => {
                    writer.write_var_bytes(bs).map_err(|e| e.to_string())?;
                },
                StackItem::Array(array) => {
                    if !serialized.insert(array as *const Array) {
                        return Err("Circular reference detected".to_string());
                    }
                    writer.write_var_int(array.len() as u64).map_err(|e| e.to_string())?;
                    unserialized.extend(array.iter().rev());
                },
                StackItem::Struct(struct_item) => {
                    if !serialized.insert(struct_item as *const Struct) {
                        return Err("Circular reference detected".to_string());
                    }
                    writer.write_var_int(struct_item.len() as u64).map_err(|e| e.to_string())?;
                    unserialized.extend(struct_item.iter().rev());
                },
                StackItem::Map(map) => {
                    if !serialized.insert(map as *const Map) {
                        return Err("Circular reference detected".to_string());
                    }
                    writer.write_var_int(map.len() as u64).map_err(|e| e.to_string())?;
                    for (key, value) in map.iter().rev() {
                        unserialized.push(value);
                        unserialized.push(key);
                    }
                },
            }

            if writer.stream_position().map_err(|e| e.to_string())? > max_size as u64 {
                return Err("Exceeded maximum size".to_string());
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Add tests here
}
