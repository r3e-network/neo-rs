//! Alias-preserving conversion from serialization values into runtime stack items.

use std::collections::HashMap;

use neo_vm_rs::{StackValue, VmOrderedDictionary};

use crate::error::{VmError, VmResult};

use super::array::Array as ArrayItem;
use super::buffer::Buffer as BufferItem;
use super::map::Map as MapItem;
use super::stack_item::{StackItem, decode_integer_bytes};
use super::struct_item::Struct as StructItem;

impl TryFrom<StackValue> for StackItem {
    type Error = VmError;

    fn try_from(value: StackValue) -> VmResult<Self> {
        StackValueConversion::default().convert(value)
    }
}

#[derive(Debug)]
struct ConvertedCompound {
    definition: StackValue,
    item: StackItem,
}

#[derive(Debug, Default)]
struct StackValueConversion {
    compounds: HashMap<u64, ConvertedCompound>,
}

impl StackValueConversion {
    fn convert(&mut self, value: StackValue) -> VmResult<StackItem> {
        if let Some(id) = stack_value_compound_id(&value) {
            if let Some(existing) = self.compounds.get(&id) {
                if stack_values_structurally_equal(&existing.definition, &value) {
                    return Ok(existing.item.clone());
                }
                return Err(VmError::invalid_operation_msg(format!(
                    "Conflicting neo-vm-rs compound definitions for id {id}"
                )));
            }
        }

        match value {
            StackValue::Integer(value) => Ok(StackItem::from_i64(value)),
            StackValue::BigInteger(bytes) => {
                let value = decode_integer_bytes(&bytes)?;
                Ok(StackItem::from_int(value))
            }
            StackValue::ByteString(bytes) => Ok(StackItem::from_byte_string(bytes)),
            StackValue::Buffer(id, bytes) => {
                let definition = StackValue::Buffer(id, bytes.clone());
                let item = StackItem::Buffer(BufferItem::with_id(bytes, local_compound_id(id)?));
                self.register(id, definition, item.clone());
                Ok(item)
            }
            StackValue::Boolean(value) => Ok(StackItem::from_bool(value)),
            StackValue::Array(id, items) => {
                let definition = StackValue::Array(id, items.clone());
                let array = ArrayItem::new_untracked_with_id(Vec::new(), local_compound_id(id)?);
                let item = StackItem::Array(array.clone());
                self.register(id, definition, item.clone());
                for child in items {
                    array.push(self.convert(child)?)?;
                }
                Ok(item)
            }
            StackValue::Struct(id, items) => {
                let definition = StackValue::Struct(id, items.clone());
                let structure =
                    StructItem::new_untracked_with_id(Vec::new(), local_compound_id(id)?);
                let item = StackItem::Struct(structure.clone());
                self.register(id, definition, item.clone());
                for child in items {
                    structure.push(self.convert(child)?)?;
                }
                Ok(item)
            }
            StackValue::Map(id, entries) => {
                let definition = StackValue::Map(id, entries.clone());
                let map = MapItem::new_untracked_with_id(
                    VmOrderedDictionary::with_capacity(entries.len()),
                    local_compound_id(id)?,
                );
                let item = StackItem::Map(map.clone());
                self.register(id, definition, item.clone());
                for (key, value) in entries {
                    map.set(self.convert(key)?, self.convert(value)?)?;
                }
                Ok(item)
            }
            StackValue::Null => Ok(StackItem::Null),
            StackValue::Pointer(_) | StackValue::Interop(_) | StackValue::Iterator(_) => {
                Err(VmError::invalid_operation_msg(format!(
                    "Cannot convert {:?} into neo-vm StackItem without host runtime identity",
                    value
                )))
            }
        }
    }

    fn register(&mut self, id: u64, definition: StackValue, item: StackItem) {
        self.compounds
            .insert(id, ConvertedCompound { definition, item });
    }
}

fn local_compound_id(id: u64) -> VmResult<usize> {
    usize::try_from(id)
        .map_err(|_| VmError::overflow("neo-vm-rs compound id does not fit local usize"))
}

const fn stack_value_compound_id(value: &StackValue) -> Option<u64> {
    match value {
        StackValue::Buffer(id, _)
        | StackValue::Array(id, _)
        | StackValue::Struct(id, _)
        | StackValue::Map(id, _) => Some(*id),
        _ => None,
    }
}

fn stack_values_structurally_equal(left: &StackValue, right: &StackValue) -> bool {
    match (left, right) {
        (StackValue::Integer(left), StackValue::Integer(right)) => left == right,
        (StackValue::BigInteger(left), StackValue::BigInteger(right))
        | (StackValue::ByteString(left), StackValue::ByteString(right)) => left == right,
        (StackValue::Buffer(left_id, left), StackValue::Buffer(right_id, right)) => {
            left_id == right_id && left == right
        }
        (StackValue::Boolean(left), StackValue::Boolean(right)) => left == right,
        (StackValue::Array(left_id, left), StackValue::Array(right_id, right))
        | (StackValue::Struct(left_id, left), StackValue::Struct(right_id, right)) => {
            left_id == right_id
                && left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| stack_values_structurally_equal(left, right))
        }
        (StackValue::Map(left_id, left), StackValue::Map(right_id, right)) => {
            left_id == right_id
                && left.len() == right.len()
                && left.iter().zip(right).all(|(left, right)| {
                    stack_values_structurally_equal(&left.0, &right.0)
                        && stack_values_structurally_equal(&left.1, &right.1)
                })
        }
        (StackValue::Interop(left), StackValue::Interop(right))
        | (StackValue::Iterator(left), StackValue::Iterator(right)) => left == right,
        (StackValue::Null, StackValue::Null) => true,
        (StackValue::Pointer(left), StackValue::Pointer(right)) => left == right,
        _ => false,
    }
}
