//! Compound operations for the Neo Virtual Machine.
//!
//! This module provides the compound operation handlers for the Neo VM.

use crate::Instruction;
use crate::OpCode;
use crate::StackItemType;
use crate::error::VmError;
use crate::error::VmResult;
use crate::execution_engine::ExecutionEngine;
use crate::jump_table::{JumpTable, register_jump_handlers, require_context};
use crate::stack_item::{Array, Map, StackItem, Struct, VmInteger};
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use std::collections::BTreeMap;

/// Validates a NEWARRAY/NEWARRAY_T/NEWSTRUCT size operand exactly like C#:
/// `var n = (int)Pop().GetInteger(); if (n < 0 || n > MaxStackSize) throw;`
/// (JumpTable.Compound.cs). The `(int)` cast faults on a count outside `i32`
/// range, and the `MaxStackSize` bound faults BEFORE allocating — so a malicious
/// large count faults cheaply instead of triggering a multi-GB `Vec` allocation
/// (the unbounded `to_i64` path could OOM-abort the node before the post-execute
/// reference-counter check). For an in-range count both paths converge.
fn collection_count(count: BigInt, max_stack_size: u32, kind: &str) -> VmResult<usize> {
    let n = count.to_i32().ok_or_else(|| {
        VmError::invalid_operation_msg(format!("The {kind} size is out of valid range"))
    })?;
    if n < 0 || n as u32 > max_stack_size {
        return Err(VmError::invalid_operation_msg(format!(
            "The {kind} size is out of valid range, {n}/[0, {max_stack_size}]."
        )));
    }
    Ok(n as usize)
}

fn pack_count(count: BigInt, available: usize, width: usize, kind: &str) -> VmResult<usize> {
    let count = count.to_i32().ok_or_else(|| {
        VmError::invalid_operation_msg(format!("The {kind} size is out of valid range"))
    })?;
    let count = usize::try_from(count).map_err(|_| {
        VmError::invalid_operation_msg(format!("The {kind} size is out of valid range"))
    })?;
    let required = count.checked_mul(width).ok_or_else(|| {
        VmError::invalid_operation_msg(format!("The {kind} size is out of valid range"))
    })?;
    if required > available {
        return Err(VmError::invalid_operation_msg(format!(
            "The {kind} size is out of valid range, {required}/[0, {available}]."
        )));
    }
    Ok(count)
}

fn normalize_index(type_name: &str, index: &BigInt, length: usize) -> VmResult<usize> {
    if let Some(idx) = index.to_usize() {
        if idx < length {
            return Ok(idx);
        }
    }

    Err(VmError::catchable_exception_msg(format!(
        "The index of {type_name} is out of range, {index}/[0, {length})."
    )))
}

/// C# `engine.Pop<PrimitiveType>()` for a collection KEY (PICKITEM, SETITEM,
/// HASKEY, REMOVE and the PACKMAP entries).
///
/// The popped key must be a `PrimitiveType` — `Integer`, `Boolean` or
/// `ByteString`. A `Buffer` is NOT a `PrimitiveType` (and neither are `Null`,
/// `Array`, `Struct`, `Map`, pointers or interop values), so the reference VM
/// throws `InvalidCastException` and FAULTS the VM UNCATCHABLY. This is NOT a
/// catchable error: it must use `invalid_type_simple`, never
/// `catchable_exception_msg` (only the in-range out-of-bounds index errors are
/// catchable, matching C#'s `CatchableException`).
fn require_primitive_key(key: &StackItem) -> VmResult<()> {
    if matches!(
        key,
        StackItem::Integer(_) | StackItem::Boolean(_) | StackItem::ByteString(_)
    ) {
        Ok(())
    } else {
        Err(VmError::invalid_type_simple(
            "key is not a PrimitiveType (C# Pop<PrimitiveType> faults)",
        ))
    }
}

fn integer_memory(value: &VmInteger) -> Vec<u8> {
    if value.is_zero() {
        Vec::new()
    } else {
        value.to_signed_bytes_le()
    }
}

fn primitive_memory(value: &StackItem) -> VmResult<Vec<u8>> {
    match value {
        StackItem::Boolean(value) => Ok(vec![u8::from(*value)]),
        StackItem::Integer(value) => Ok(integer_memory(value)),
        StackItem::ByteString(bytes) => Ok(bytes.clone()),
        _ => Err(VmError::invalid_type_simple("Expected PrimitiveType")),
    }
}

fn pick_byte_sequence_item(bytes: &[u8], index: usize) -> VmResult<StackItem> {
    bytes
        .get(index)
        .copied()
        .map(|byte| StackItem::from_i64(i64::from(byte)))
        .ok_or_else(|| VmError::invalid_operation_msg("Index out of range"))
}

mod before543;

pub(crate) use before543::{
    has_key_before543, pick_item_before543, remove_before543, set_item_before543,
};

/// Registers the compound operation handlers.
pub fn register_handlers<S>(jump_table: &mut JumpTable<S>) {
    register_jump_handlers![
        jump_table;
        OpCode::NEWARRAY0 => new_array0,
        OpCode::NEWARRAY => new_array,
        OpCode::NEWARRAY_T => new_array_t,
        OpCode::NEWSTRUCT0 => new_struct0,
        OpCode::NEWSTRUCT => new_struct,
        OpCode::NEWMAP => new_map,
        OpCode::APPEND => append,
        OpCode::REVERSEITEMS => reverse,
        OpCode::REMOVE => remove,
        OpCode::CLEARITEMS => clear_items,
        OpCode::POPITEM => pop_item,
        OpCode::HASKEY => has_key,
        OpCode::KEYS => keys,
        OpCode::VALUES => values,
        OpCode::PACKMAP => pack_map,
        OpCode::PACKSTRUCT => pack_struct,
        OpCode::PACK => pack,
        OpCode::UNPACK => unpack,
        OpCode::PICKITEM => pick_item,
        OpCode::SETITEM => set_item,
        OpCode::SIZE => size,
    ];
}

/// Implements the NEWARRAY0 operation.
fn new_array0<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    let context = require_context(engine)?;

    let array = Array::new(Vec::new(), Some(context.reference_counter().clone()))?;
    context.push(StackItem::Array(array))?;

    Ok(())
}

/// Implements the NEWARRAY operation.
fn new_array<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    let max_stack_size = engine.limits().max_stack_size;
    let context = require_context(engine)?;

    // C# bounds the count by MaxStackSize and faults before allocating.
    let count = collection_count(super::get_integer(context.pop()?)?, max_stack_size, "array")?;

    let array = Array::new(
        vec![StackItem::Null; count],
        Some(context.reference_counter().clone()),
    )?;
    context.push(StackItem::Array(array))?;

    Ok(())
}

/// Implements the `NewarrayT` operation.
fn new_array_t<S>(engine: &mut ExecutionEngine<S>, instruction: &Instruction) -> VmResult<()> {
    let max_stack_size = engine.limits().max_stack_size;
    let context = require_context(engine)?;

    // C# bounds the count by MaxStackSize and faults before reading the type/allocating.
    let count = collection_count(super::get_integer(context.pop()?)?, max_stack_size, "array")?;

    // Get the type from the instruction
    let type_byte = instruction
        .operand()
        .first()
        .copied()
        .ok_or_else(|| VmError::invalid_instruction_msg("Missing type operand"))?;

    let item_type = StackItemType::from_byte(type_byte).ok_or_else(|| {
        VmError::invalid_instruction_msg(format!("Invalid type: {type_byte:#04x}"))
    })?;

    let default_item = match item_type {
        StackItemType::Boolean => StackItem::false_value(),
        StackItemType::Integer => StackItem::from_i64(0),
        StackItemType::ByteString => StackItem::from_byte_string(Vec::new()),
        _ => StackItem::Null,
    };
    let array = Array::new(
        vec![default_item; count],
        Some(context.reference_counter().clone()),
    )?;
    context.push(StackItem::Array(array))?;

    Ok(())
}

/// Implements the NEWSTRUCT0 operation.
fn new_struct0<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    let context = require_context(engine)?;

    let structure = Struct::new(Vec::new(), Some(context.reference_counter().clone()))?;
    context.push(StackItem::Struct(structure))?;

    Ok(())
}

/// Implements the NEWSTRUCT operation.
fn new_struct<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    let max_stack_size = engine.limits().max_stack_size;
    let context = require_context(engine)?;

    // C# bounds the count by MaxStackSize and faults before allocating.
    let count = collection_count(
        super::get_integer(context.pop()?)?,
        max_stack_size,
        "struct",
    )?;

    let structure = Struct::new(
        vec![StackItem::Null; count],
        Some(context.reference_counter().clone()),
    )?;
    context.push(StackItem::Struct(structure))?;

    Ok(())
}

/// Implements the NEWMAP operation.
fn new_map<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    let context = require_context(engine)?;

    let map = Map::new(BTreeMap::new(), Some(context.reference_counter().clone()))?;
    context.push(StackItem::Map(map))?;

    Ok(())
}

/// Implements the APPEND operation.
fn append<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    let limits = *engine.limits();
    let context = require_context(engine)?;

    let mut item = context.pop()?;
    let collection = context.pop()?;

    if matches!(item, StackItem::Struct(_)) {
        item = item.deep_copy(&limits)?;
    }

    match collection {
        StackItem::Array(array) => {
            array.push(item)?;
            // APPEND pops both operands and does not push the array back (Pop 2, Push 0).
        }
        StackItem::Struct(structure) => {
            structure.push(item)?;
        }
        _ => {
            return Err(VmError::invalid_type_simple(
                "Expected Array, Struct, or Map",
            ));
        }
    }

    Ok(())
}

/// Implements the REVERSE operation.
fn reverse<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = require_context(engine)?;

    // Pop the array from the stack
    let array = context.pop()?;

    // Reverse the array
    match array {
        StackItem::Array(array) => {
            array.reverse_items()?;
        }
        StackItem::Struct(structure) => {
            structure.reverse_items()?;
        }
        StackItem::Buffer(buffer) => {
            buffer.with_data_mut(|data| data.reverse());
        }
        _ => {
            return Err(VmError::invalid_type_simple(
                "Expected Array, Struct, or Buffer",
            ));
        }
    }

    Ok(())
}

/// Implements the REMOVE operation.
fn remove<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = require_context(engine)?;

    // Pop the key and collection from the stack
    let key = context.pop()?;
    require_primitive_key(&key)?;
    let collection = context.pop()?;

    match collection {
        StackItem::Array(array) => {
            let index = key
                .as_int()?
                .to_usize()
                .ok_or_else(|| VmError::invalid_operation_msg("Invalid array index"))?;
            if index >= array.len() {
                return Err(VmError::invalid_operation_msg(format!(
                    "Index out of range: {index}"
                )));
            }
            let _ = array.remove(index)?;
        }
        StackItem::Struct(structure) => {
            let index = key
                .as_int()?
                .to_usize()
                .ok_or_else(|| VmError::invalid_operation_msg("Invalid struct index"))?;
            if index >= structure.len() {
                return Err(VmError::invalid_operation_msg(format!(
                    "Index out of range: {index}"
                )));
            }
            let _ = structure.remove(index)?;
        }
        StackItem::Map(map) => {
            if map.contains_key(&key)? {
                let _ = map.remove(&key)?;
            }
        }
        _ => {
            return Err(VmError::invalid_type_simple(
                "Expected Array, Struct, or Map",
            ));
        }
    }

    Ok(())
}

/// Implements the CLEARITEMS operation.
fn clear_items<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = require_context(engine)?;

    // Pop the collection from the stack
    let collection = context.pop()?;

    // Clear the collection
    match collection {
        StackItem::Array(array) => {
            array.clear()?;
        }
        StackItem::Struct(structure) => {
            structure.clear()?;
        }
        StackItem::Map(map) => {
            map.clear()?;
        }
        _ => {
            return Err(VmError::invalid_type_simple(
                "Expected Array, Struct, or Map",
            ));
        }
    }

    Ok(())
}

/// Implements the POPITEM operation.
fn pop_item<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = require_context(engine)?;

    // Pop the collection from the stack
    let collection = context.pop()?;

    match collection {
        StackItem::Array(array) => {
            let popped_item = array.pop()?;
            context.push(popped_item)?;
        }
        StackItem::Struct(structure) => {
            let popped_item = structure.pop()?;
            context.push(popped_item)?;
        }
        _ => return Err(VmError::invalid_type_simple("Expected Array or Struct")),
    }

    Ok(())
}

/// Implements the HASKEY operation.
fn has_key<S>(engine: &mut ExecutionEngine<S>, instruction: &Instruction) -> VmResult<()> {
    // C# HasKey faults when the index is out of `[0, MaxItemSize)` BEFORE
    // comparing against the collection's actual length (VMArray/Buffer/ByteString).
    let max_item_size = engine.limits().max_item_size as usize;
    let context = require_context(engine)?;

    // Pop the key and collection from the stack
    let key = context.pop()?;
    require_primitive_key(&key)?;
    let collection = context.pop()?;

    let invalid_index = |index: &BigInt| {
        VmError::invalid_operation_msg(format!(
            "The index {index} is invalid for OpCode {:?}",
            instruction.opcode()
        ))
    };

    let result = match &collection {
        StackItem::Array(array) => {
            let index = key.as_int()?;
            if index < BigInt::from(0_u8) || index >= BigInt::from(max_item_size) {
                return Err(invalid_index(&index));
            }
            index.to_usize().is_some_and(|index| index < array.len())
        }
        StackItem::Struct(structure) => {
            let index = key.as_int()?;
            if index < BigInt::from(0_u8) || index >= BigInt::from(max_item_size) {
                return Err(invalid_index(&index));
            }
            index
                .to_usize()
                .is_some_and(|index| index < structure.len())
        }
        StackItem::Map(map) => map.contains_key(&key)?,
        StackItem::ByteString(bytes) => {
            let index = key.as_int()?;
            if index < BigInt::from(0_u8) || index >= BigInt::from(max_item_size) {
                return Err(invalid_index(&index));
            }
            index.to_usize().is_some_and(|index| index < bytes.len())
        }
        StackItem::Buffer(buffer) => {
            let index = key.as_int()?;
            if index < BigInt::from(0_u8) || index >= BigInt::from(max_item_size) {
                return Err(invalid_index(&index));
            }
            index.to_usize().is_some_and(|index| index < buffer.len())
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

/// Implements the KEYS operation.
fn keys<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = require_context(engine)?;

    // Pop the map from the stack
    let map = context.pop()?;

    // Get the keys from the map
    match map {
        StackItem::Map(map) => {
            let keys: Vec<StackItem> =
                map.with_items(|items| items.iter().map(|(k, _)| k.clone()).collect());
            let array = Array::new(keys, Some(context.reference_counter().clone()))?;
            context.push(StackItem::Array(array))?;
        }
        _ => return Err(VmError::invalid_type_simple("Expected Map")),
    }

    Ok(())
}

/// Implements the VALUES operation.
///
/// C# `Values` (JumpTable.Compound.cs:343-358) accepts BOTH an Array (including a
/// `Struct`, since `Struct : Array`) and a Map as the source, deep-clones each
/// `Struct` element via `Struct.Clone(engine.Limits)` (which faults past the
/// per-clone subitem limit), and adds every other element by reference. The Rust
/// handler previously accepted only a Map and shallow-cloned its values, so a
/// VALUES over an Array/Struct faulted and the result aliased the source Structs.
fn values<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    let limits = *engine.limits();
    let context = require_context(engine)?;

    let source = context.pop()?;
    let source_items: Vec<StackItem> = match source {
        StackItem::Array(array) => array.items(),
        StackItem::Struct(structure) => structure.items(),
        StackItem::Map(map) => {
            map.with_items(|items| items.iter().map(|(_, v)| v.clone()).collect())
        }
        _ => {
            return Err(VmError::invalid_type_simple(
                "Invalid type for VALUES (expected Array, Struct or Map)",
            ));
        }
    };

    // C#: `if (item is Struct s) newArray.Add(s.Clone(engine.Limits)); else newArray.Add(item);`
    let mut values = Vec::with_capacity(source_items.len());
    for item in source_items {
        if matches!(item, StackItem::Struct(_)) {
            values.push(item.deep_copy(&limits)?);
        } else {
            values.push(item);
        }
    }

    let array = Array::new(values, Some(context.reference_counter().clone()))?;
    context.push(StackItem::Array(array))?;
    Ok(())
}

/// Implements the PACKMAP operation.
fn pack_map<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    let context = require_context(engine)?;

    let count_item = context.pop()?;
    let available = context.evaluation_stack().len();
    let count = pack_count(super::get_integer(count_item)?, available, 2, "map")?;

    let map_item = Map::new(BTreeMap::new(), Some(context.reference_counter().clone()))?;

    for _ in 0..count {
        let key = context.pop()?;
        require_primitive_key(&key)?;
        let value = context.pop()?;
        map_item.set(key, value)?;
    }

    context.push(StackItem::Map(map_item))?;

    Ok(())
}

/// Implements the PACKSTRUCT operation.
fn pack_struct<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    let context = require_context(engine)?;

    let count_item = context.pop()?;
    let available = context.evaluation_stack().len();
    let count = pack_count(super::get_integer(count_item)?, available, 1, "struct")?;

    let mut items = Vec::with_capacity(count);
    for _ in 0..count {
        items.push(context.pop()?);
    }

    let structure = Struct::new(items, Some(context.reference_counter().clone()))?;
    context.push(StackItem::Struct(structure))?;

    Ok(())
}

/// Implements the PACK operation.
fn pack<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    let context = require_context(engine)?;

    let count_item = context.pop()?;
    let available = context.evaluation_stack().len();
    let count = pack_count(super::get_integer(count_item)?, available, 1, "array")?;

    // Create a new array
    let mut items = Vec::with_capacity(count);
    for _ in 0..count {
        items.push(context.pop()?);
    }

    let array = Array::new(items, Some(context.reference_counter().clone()))?;
    context.push(StackItem::Array(array))?;

    Ok(())
}

/// Implements the UNPACK operation.
fn unpack<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = require_context(engine)?;

    // Pop the array from the stack
    let array = context.pop()?;

    // Unpack the array - collect items first to avoid holding locks during push
    match array {
        StackItem::Array(array) => {
            let len = array.len();
            let items: Vec<StackItem> =
                array.with_items(|items| items.iter().rev().cloned().collect());
            for item in items {
                context.push(item)?;
            }
            context.push(StackItem::from_int(len))?;
        }
        StackItem::Struct(structure) => {
            let len = structure.len();
            let items: Vec<StackItem> =
                structure.with_items(|items| items.iter().rev().cloned().collect());
            for item in items {
                context.push(item)?;
            }
            context.push(StackItem::from_int(len))?;
        }
        StackItem::Map(map) => {
            let len = map.len();
            let pairs: Vec<(StackItem, StackItem)> = map.with_items(|items| {
                items
                    .iter()
                    .rev()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            });
            for (key, value) in pairs {
                context.push(value)?;
                context.push(key)?;
            }
            context.push(StackItem::from_int(len))?;
        }
        _ => return Err(VmError::invalid_type_simple("Expected Array or Struct")),
    }

    Ok(())
}

/// Implements the PICKITEM operation.
fn pick_item<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    let context = require_context(engine)?;

    let key = context.pop()?;
    require_primitive_key(&key)?;
    let collection = context.pop()?;

    let result = match collection {
        StackItem::Array(array) => {
            let idx = normalize_index("VMArray", &key.as_integer()?, array.len())?;
            array
                .get(idx)
                .ok_or_else(|| VmError::invalid_operation_msg("Index out of range"))?
        }
        StackItem::Struct(structure) => {
            let idx = normalize_index("Struct", &key.as_integer()?, structure.len())?;
            structure.get(idx)?
        }
        StackItem::Map(map) => map.get(&key)?,
        StackItem::ByteString(bytes) => {
            let idx = normalize_index("PrimitiveType", &key.as_integer()?, bytes.len())?;
            pick_byte_sequence_item(&bytes, idx)?
        }
        // C# Neo VM PICKITEM on PrimitiveType reads the bytewise GetSpan()
        // representation. Boolean false is the one-byte span [0], while
        // Integer zero is the empty span.
        item @ (StackItem::Integer(_) | StackItem::Boolean(_)) => {
            let bytes = primitive_memory(&item)?;
            let idx = normalize_index("PrimitiveType", &key.as_integer()?, bytes.len())?;
            pick_byte_sequence_item(&bytes, idx)?
        }
        StackItem::Buffer(buffer) => {
            let idx = normalize_index("Buffer", &key.as_integer()?, buffer.len())?;
            StackItem::from_i64(i64::from(buffer.get(idx)?))
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

/// Implements the SETITEM operation.
fn set_item<S>(engine: &mut ExecutionEngine<S>, instruction: &Instruction) -> VmResult<()> {
    let limits = *engine.limits();
    let context = require_context(engine)?;

    let mut value = context.pop()?;
    if matches!(value, StackItem::Struct(_)) {
        value = value.deep_copy(&limits)?;
    }
    let key = context.pop()?;
    require_primitive_key(&key)?;
    let collection = context.pop()?;

    match collection {
        StackItem::Array(array) => {
            if let Some(rc) = array.reference_counter() {
                value.attach_reference_counter(&rc)?;
            }
            let idx = normalize_index("VMArray", &key.as_integer()?, array.len())?;
            array.set(idx, value)?;
        }
        StackItem::Struct(structure) => {
            if let Some(rc) = structure.reference_counter() {
                value.attach_reference_counter(&rc)?;
            }
            let idx = normalize_index("Struct", &key.as_integer()?, structure.len())?;
            structure.set(idx, value)?;
        }
        StackItem::Map(map) => {
            if let Some(rc) = map.reference_counter() {
                value.attach_reference_counter(&rc)?;
            }
            map.set(key, value)?;
        }
        StackItem::Buffer(buffer) => {
            let idx = normalize_index("Buffer", &key.as_integer()?, buffer.len())?;
            let byte = if matches!(
                value,
                StackItem::Integer(_) | StackItem::Boolean(_) | StackItem::ByteString(_)
            ) {
                value.as_integer()
            } else {
                Err(VmError::invalid_type_simple("Expected PrimitiveType"))
            }
            .map_err(|_| {
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

/// Implements the SIZE operation.
fn size<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = require_context(engine)?;

    // Pop the collection from the stack
    let collection = context.pop()?;

    // Get the size of the collection.
    // C# Neo VM SIZE accepts CompoundType (Count), PrimitiveType (Size), and Buffer (Size).
    // PrimitiveType subclasses are ByteString, Integer, Boolean — all expose Size.
    let size = match collection {
        StackItem::Array(array) => array.len(),
        StackItem::Struct(structure) => structure.len(),
        StackItem::Map(map) => map.len(),
        StackItem::ByteString(data) => data.len(),
        StackItem::Buffer(data) => data.len(),
        StackItem::Integer(value) => integer_memory(&value).len(),
        StackItem::Boolean(_) => 1,
        _ => {
            return Err(VmError::invalid_type_simple(
                "Expected Array, Struct, Map, ByteString, Buffer, Integer, or Boolean",
            ));
        }
    };

    // Push the size onto the stack
    context.push(StackItem::from_int(size))?;

    Ok(())
}

#[cfg(test)]
mod local_stack_item_tests {
    use super::*;
    use crate::script::Script;

    fn engine_with_stack(items: Vec<StackItem>) -> ExecutionEngine {
        let mut engine = ExecutionEngine::<()>::new(None);
        engine
            .load_script(Script::new_relaxed(vec![OpCode::RET.byte()]), -1, 0)
            .expect("load test script");
        let context = engine.current_context_mut().expect("current context");
        for item in items {
            context.push(item).expect("push test item");
        }
        engine
    }

    fn pop(engine: &mut ExecutionEngine) -> StackItem {
        engine
            .current_context_mut()
            .expect("current context")
            .pop()
            .expect("result item")
    }

    #[test]
    fn new_array_t_builds_tracked_local_defaults() {
        let mut engine = engine_with_stack(vec![StackItem::from_i64(2)]);
        let instruction = Instruction::new(OpCode::NEWARRAY_T, &[StackItemType::Boolean.to_byte()]);

        new_array_t(&mut engine, &instruction).expect("NEWARRAY_T succeeds");

        match pop(&mut engine) {
            StackItem::Array(array) => {
                assert!(array.reference_counter().is_some());
                assert!(
                    array
                        .items()
                        .iter()
                        .all(|item| matches!(item, StackItem::Boolean(false)))
                );
            }
            other => panic!("expected Array, got {other:?}"),
        }
    }

    #[test]
    fn primitive_pick_and_size_use_neovm_memory() {
        let mut engine =
            engine_with_stack(vec![StackItem::from_bool(false), StackItem::from_i64(0)]);
        pick_item(&mut engine, &Instruction::new(OpCode::PICKITEM, &[]))
            .expect("PICKITEM succeeds");
        assert_eq!(pop(&mut engine).as_int().unwrap(), BigInt::from(0));

        let mut engine = engine_with_stack(vec![StackItem::from_i64(0)]);
        size(&mut engine, &Instruction::new(OpCode::SIZE, &[])).expect("SIZE succeeds");
        assert_eq!(pop(&mut engine).as_int().unwrap(), BigInt::from(0));

        let mut engine = engine_with_stack(vec![StackItem::from_bool(false)]);
        size(&mut engine, &Instruction::new(OpCode::SIZE, &[])).expect("SIZE succeeds");
        assert_eq!(pop(&mut engine).as_int().unwrap(), BigInt::from(1));
    }

    #[test]
    fn pack_counts_fault_before_unbounded_allocation() {
        assert!(pack_count(BigInt::from(i64::MAX), 0, 1, "array").is_err());
        assert!(pack_count(BigInt::from(2), 3, 2, "map").is_err());
        assert_eq!(pack_count(BigInt::from(2), 4, 2, "map").unwrap(), 2);
    }

    #[test]
    fn remove_missing_map_key_is_a_noop() {
        let map = Map::new(BTreeMap::new(), None).expect("empty map");
        let mut engine = engine_with_stack(vec![
            StackItem::Map(map),
            StackItem::from_byte_string(b"missing".to_vec()),
        ]);

        remove(&mut engine, &Instruction::new(OpCode::REMOVE, &[]))
            .expect("missing map key is ignored");
    }
}

#[cfg(test)]
#[path = "../tests/jump_table/compound.rs"]
mod tests;
