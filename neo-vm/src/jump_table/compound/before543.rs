//! Pre-HF_Gorgon compound operation variants.
//!
//! The neo-vm#543 fix changed index validation for HASKEY/PICKITEM/SETITEM/REMOVE
//! to bounds-check the full `BigInteger` index. These pre-543 handlers, kept for
//! the NotGorgon / NotEchidna tables, cast the index to a 32-bit `int` first, so
//! an index outside `i32` range faults uncatchably before normal bounds checks.

use crate::error::{VmError, VmResult};
use crate::execution_engine::ExecutionEngine;
use crate::stack_item::StackItem;
use neo_vm_rs::Instruction;
use num_traits::ToPrimitive;

use super::pick_byte_sequence_item;

/// C# `(int)key.GetInteger()`: a 32-bit index whose overflow is an uncatchable fault.
fn before543_index(key: &StackItem) -> VmResult<i32> {
    key.as_int()?
        .to_i32()
        .ok_or_else(|| VmError::invalid_operation_msg("Index overflow"))
}

/// Pre-543 catchable bounded index (PICKITEM/SETITEM): out-of-range is a
/// `CatchableException`, but an `i32`-overflowing index faults uncatchably first.
fn before543_checked_index(type_name: &str, key: &StackItem, length: usize) -> VmResult<usize> {
    let index = before543_index(key)?;
    if index < 0 || index as usize >= length {
        return Err(VmError::catchable_exception_msg(format!(
            "The index of {type_name} is out of range, {index}/[0, {length})."
        )));
    }
    Ok(index as usize)
}

/// REMOVE, pre-543 (C# `Remove_Before543`): array/struct out-of-range is an
/// uncatchable `InvalidOperationException`.
pub(crate) fn remove_before543(
    engine: &mut ExecutionEngine,
    _instruction: &Instruction,
) -> VmResult<()> {
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
    let key = context.pop()?;
    let collection = context.pop()?;
    match collection {
        StackItem::Array(array) => {
            let index = before543_index(&key)?;
            if index < 0 || index as usize >= array.len() {
                return Err(VmError::invalid_operation_msg(format!(
                    "The index of VMArray is out of range, {index}/[0, {}).",
                    array.len()
                )));
            }
            let _ = array.remove(index as usize)?;
        }
        StackItem::Struct(structure) => {
            let index = before543_index(&key)?;
            if index < 0 || index as usize >= structure.len() {
                return Err(VmError::invalid_operation_msg(format!(
                    "The index of VMArray is out of range, {index}/[0, {}).",
                    structure.len()
                )));
            }
            let _ = structure.remove(index as usize)?;
        }
        StackItem::Map(map) => {
            let _ = map.remove(&key)?;
        }
        _ => {
            return Err(VmError::invalid_type_simple(
                "Expected Array, Struct, or Map",
            ));
        }
    }
    Ok(())
}

/// HASKEY, pre-543 (C# `HasKey_Before543`): a negative index is an uncatchable
/// `InvalidOperationException`; an in-range index pushes the membership bool.
pub(crate) fn has_key_before543(
    engine: &mut ExecutionEngine,
    instruction: &Instruction,
) -> VmResult<()> {
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
    let key = context.pop()?;
    let collection = context.pop()?;

    let negative = |index: i32| {
        VmError::invalid_operation_msg(format!(
            "The negative index {index} is invalid for OpCode {:?}.",
            instruction.opcode()
        ))
    };
    let result = match &collection {
        StackItem::Array(array) => {
            let index = before543_index(&key)?;
            if index < 0 {
                return Err(negative(index));
            }
            (index as usize) < array.len()
        }
        StackItem::Struct(structure) => {
            let index = before543_index(&key)?;
            if index < 0 {
                return Err(negative(index));
            }
            (index as usize) < structure.len()
        }
        StackItem::Map(map) => map.contains_key(&key)?,
        StackItem::Buffer(buffer) => {
            let index = before543_index(&key)?;
            if index < 0 {
                return Err(negative(index));
            }
            (index as usize) < buffer.len()
        }
        StackItem::ByteString(bytes) => {
            let index = before543_index(&key)?;
            if index < 0 {
                return Err(negative(index));
            }
            (index as usize) < bytes.len()
        }
        _ => {
            return Err(VmError::invalid_type_simple(
                "Expected Array, Struct, Map, ByteString, or Buffer",
            ));
        }
    };
    context.push(StackItem::from_bool(result))?;
    Ok(())
}

/// PICKITEM, pre-543 (C# `PickItem_Before543`): out-of-range is a
/// `CatchableException`, but an `i32`-overflowing index faults uncatchably.
pub(crate) fn pick_item_before543(
    engine: &mut ExecutionEngine,
    _instruction: &Instruction,
) -> VmResult<()> {
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
    let key = context.pop()?;
    let collection = context.pop()?;

    let result = match collection {
        StackItem::Array(array) => {
            let idx = before543_checked_index("VMArray", &key, array.len())?;
            array
                .get(idx)
                .ok_or_else(|| VmError::invalid_operation_msg("Index out of range"))?
        }
        StackItem::Struct(structure) => {
            let idx = before543_checked_index("Struct", &key, structure.len())?;
            structure.get(idx)?
        }
        StackItem::Map(map) => map.get(&key)?,
        StackItem::ByteString(bytes) => {
            let idx = before543_checked_index("PrimitiveType", &key, bytes.len())?;
            pick_byte_sequence_item(neo_vm_rs::StackValue::ByteString(bytes.clone()), idx)?
        }
        item @ (StackItem::Integer(_) | StackItem::Boolean(_)) => {
            let index = before543_index(&key)?;
            if index < 0 {
                return Err(VmError::catchable_exception_msg(format!(
                    "The index of PrimitiveType is out of range, {index}/[0, ?)."
                )));
            }
            pick_byte_sequence_item(neo_vm_rs::StackValue::try_from(item)?, index as usize)?
        }
        StackItem::Buffer(buffer) => {
            let idx = before543_checked_index("Buffer", &key, buffer.len())?;
            pick_byte_sequence_item(neo_vm_rs::StackValue::Buffer(0, buffer.data()), idx)?
        }
        _ => {
            return Err(VmError::invalid_type_simple(
                "Expected Array, Struct, Map, ByteString, or Buffer",
            ));
        }
    };
    context.push(result)?;
    Ok(())
}

/// SETITEM, pre-543 (C# `SetItem_Before543`): out-of-range is a
/// `CatchableException`, but an `i32`-overflowing index faults uncatchably.
pub(crate) fn set_item_before543(
    engine: &mut ExecutionEngine,
    instruction: &Instruction,
) -> VmResult<()> {
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
    let mut value = context.pop()?;
    let key = context.pop()?;
    let collection = context.pop()?;

    if matches!(value, StackItem::Struct(_)) {
        value = value.deep_clone();
    }

    match collection {
        StackItem::Array(array) => {
            if let Some(rc) = array.reference_counter() {
                value.attach_reference_counter(&rc)?;
            }
            let idx = before543_checked_index("VMArray", &key, array.len())?;
            array.set(idx, value)?;
        }
        StackItem::Struct(structure) => {
            if let Some(rc) = structure.reference_counter() {
                value.attach_reference_counter(&rc)?;
            }
            let idx = before543_checked_index("Struct", &key, structure.len())?;
            structure.set(idx, value)?;
        }
        StackItem::Map(map) => {
            if let Some(rc) = map.reference_counter() {
                value.attach_reference_counter(&rc)?;
            }
            map.set(key, value)?;
        }
        StackItem::Buffer(buffer) => {
            let idx = before543_checked_index("Buffer", &key, buffer.len())?;
            let byte = value.as_integer().map_err(|_| {
                VmError::invalid_operation_msg(format!(
                    "Only primitive type values can be set in Buffer in {:?}.",
                    instruction.opcode()
                ))
            })?;
            let byte = byte.to_i32().ok_or_else(|| {
                VmError::invalid_operation_msg(format!(
                    "Only primitive type values can be set in Buffer in {:?}.",
                    instruction.opcode()
                ))
            })?;
            if byte < i32::from(i8::MIN) || byte > i32::from(u8::MAX) {
                return Err(VmError::invalid_operation_msg(format!(
                    "Overflow in {:?}, {byte} is not a byte type.",
                    instruction.opcode()
                )));
            }
            buffer.set(idx, byte as u8)?;
        }
        _ => {
            return Err(VmError::invalid_type_simple(
                "Expected Array, Struct, Map, or Buffer",
            ));
        }
    }
    Ok(())
}
