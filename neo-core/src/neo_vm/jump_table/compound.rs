//! Compound operations for the Neo Virtual Machine.
//!
//! This module provides the compound operation handlers for the Neo VM.

use crate::neo_vm::error::VmError;
use crate::neo_vm::error::VmResult;
use crate::neo_vm::execution_engine::ExecutionEngine;
use crate::neo_vm::instruction::Instruction;
use crate::neo_vm::jump_table::JumpTable;
use crate::neo_vm::stack_item::primitive_type::PrimitiveTypeExt;
use crate::neo_vm::stack_item::{Array, Map, StackItem, StackItemType, Struct};
use neo_vm_rs::OpCode;
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use std::collections::BTreeMap;

fn collection_stack_item(value: Result<neo_vm_rs::StackValue, String>) -> VmResult<StackItem> {
    StackItem::try_from(value.map_err(VmError::invalid_operation_msg)?)
}

fn stack_value_byte_string_len(value: neo_vm_rs::StackValue) -> VmResult<usize> {
    stack_value_byte_string_bytes(value).map(|bytes| bytes.len())
}

fn stack_value_byte_string_bytes(value: neo_vm_rs::StackValue) -> VmResult<Vec<u8>> {
    value
        .to_byte_string_bytes()
        .ok_or_else(|| VmError::invalid_type_simple("Cannot convert to ByteArray"))
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

fn byte_sequence_key_value(key: &StackItem) -> VmResult<neo_vm_rs::StackValue> {
    let index = key
        .as_int()?
        .to_i64()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid index"))?;
    Ok(neo_vm_rs::StackValue::Integer(index))
}

fn byte_sequence_has_key(value: neo_vm_rs::StackValue, key: &StackItem) -> VmResult<bool> {
    let key = byte_sequence_key_value(key)?;
    neo_vm_rs::semantics::collections::has_key(&value, &key).map_err(VmError::invalid_operation_msg)
}

fn pick_byte_sequence_item(value: neo_vm_rs::StackValue, index: usize) -> VmResult<StackItem> {
    let key = neo_vm_rs::StackValue::Integer(
        i64::try_from(index).map_err(|_| VmError::invalid_operation_msg("Invalid index"))?,
    );
    StackItem::try_from(
        neo_vm_rs::semantics::collections::pick_item(&value, &key)
            .map_err(VmError::invalid_operation_msg)?,
    )
}

/// Registers the compound operation handlers.
pub fn register_handlers(jump_table: &mut JumpTable) {
    jump_table.register(OpCode::NEWARRAY0, new_array0);
    jump_table.register(OpCode::NEWARRAY, new_array);
    jump_table.register(OpCode::NEWARRAY_T, new_array_t);
    jump_table.register(OpCode::NEWSTRUCT0, new_struct0);
    jump_table.register(OpCode::NEWSTRUCT, new_struct);
    jump_table.register(OpCode::NEWMAP, new_map);
    jump_table.register(OpCode::APPEND, append);
    jump_table.register(OpCode::REVERSEITEMS, reverse);
    jump_table.register(OpCode::REMOVE, remove);
    jump_table.register(OpCode::CLEARITEMS, clear_items);
    jump_table.register(OpCode::POPITEM, pop_item);
    jump_table.register(OpCode::HASKEY, has_key);
    jump_table.register(OpCode::KEYS, keys);
    jump_table.register(OpCode::VALUES, values);
    jump_table.register(OpCode::PACKMAP, pack_map);
    jump_table.register(OpCode::PACKSTRUCT, pack_struct);
    jump_table.register(OpCode::PACK, pack);
    jump_table.register(OpCode::UNPACK, unpack);
    jump_table.register(OpCode::PICKITEM, pick_item);
    jump_table.register(OpCode::SETITEM, set_item);
    jump_table.register(OpCode::SIZE, size);
}

/// Implements the NEWARRAY0 operation.
fn new_array0(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    let array = collection_stack_item(neo_vm_rs::semantics::collections::new_array(0))?;
    context.push(array)?;

    Ok(())
}

/// Implements the NEWARRAY operation.
fn new_array(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the count from the stack
    let count = context
        .pop()?
        .into_int()?
        .to_i64()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid array size"))?;

    let array = collection_stack_item(neo_vm_rs::semantics::collections::new_array(count))?;
    context.push(array)?;

    Ok(())
}

/// Implements the `NewarrayT` operation.
fn new_array_t(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the count from the stack
    let count = context
        .pop()?
        .into_int()?
        .to_i64()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid array size"))?;

    // Get the type from the instruction
    let type_byte = instruction
        .operand()
        .first()
        .copied()
        .ok_or_else(|| VmError::invalid_instruction_msg("Missing type operand"))?;

    if StackItemType::from_byte(type_byte).is_none() {
        return Err(VmError::invalid_instruction_msg(format!(
            "Invalid type: {type_byte:#04x}"
        )));
    }

    let array = collection_stack_item(neo_vm_rs::semantics::collections::new_array_t(
        count, type_byte,
    ))?;
    context.push(array)?;

    Ok(())
}

/// Implements the NEWSTRUCT0 operation.
fn new_struct0(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    let structure = collection_stack_item(neo_vm_rs::semantics::collections::new_struct(0))?;
    context.push(structure)?;

    Ok(())
}

/// Implements the NEWSTRUCT operation.
fn new_struct(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the count from the stack
    let count = context
        .pop()?
        .into_int()?
        .to_i64()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid struct size"))?;

    let structure = collection_stack_item(neo_vm_rs::semantics::collections::new_struct(count))?;
    context.push(structure)?;

    Ok(())
}

/// Implements the NEWMAP operation.
fn new_map(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    let map_value = neo_vm_rs::semantics::collections::pack_map(Vec::new());
    let map = collection_stack_item(Ok(map_value))?;
    context.push(map)?;

    Ok(())
}

/// Implements the APPEND operation.
fn append(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    let mut item = context.pop()?;
    let collection = context.pop()?;

    if matches!(item, StackItem::Struct(_)) {
        item = item.deep_clone();
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
            ))
        }
    }

    Ok(())
}

/// Implements the REVERSE operation.
fn reverse(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

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
fn remove(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the key and collection from the stack
    let key = context.pop()?;
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

/// Implements the CLEARITEMS operation.
fn clear_items(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

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
fn pop_item(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

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
fn has_key(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the key and collection from the stack
    let key = context.pop()?;
    let collection = context.pop()?;

    let result = match &collection {
        StackItem::Array(array) => {
            let index = key
                .as_int()?
                .to_usize()
                .ok_or_else(|| VmError::invalid_operation_msg("Invalid array index"))?;
            index < array.len()
        }
        StackItem::Struct(structure) => {
            let index = key
                .as_int()?
                .to_usize()
                .ok_or_else(|| VmError::invalid_operation_msg("Invalid struct index"))?;
            index < structure.len()
        }
        StackItem::Map(map) => map.contains_key(&key)?,
        StackItem::ByteString(bytes) => {
            byte_sequence_has_key(neo_vm_rs::StackValue::ByteString(bytes.clone()), &key)?
        }
        StackItem::Buffer(buffer) => {
            byte_sequence_has_key(neo_vm_rs::StackValue::Buffer(buffer.data()), &key)?
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
fn keys(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

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
fn values(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the map from the stack
    let map = context.pop()?;

    // Get the values from the map
    match map {
        StackItem::Map(map) => {
            let values: Vec<StackItem> =
                map.with_items(|items| items.iter().map(|(_, v)| v.clone()).collect());
            let array = Array::new(values, Some(context.reference_counter().clone()))?;
            context.push(StackItem::Array(array))?;
        }
        _ => return Err(VmError::invalid_type_simple("Expected Map")),
    }

    Ok(())
}

/// Implements the PACKMAP operation.
fn pack_map(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the count from the stack
    let count = context
        .pop()?
        .into_int()?
        .to_usize()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid map size"))?;

    let map_item = Map::new(BTreeMap::new(), Some(context.reference_counter().clone()))?;

    for _ in 0..count {
        let key = context.pop()?;
        let value = context.pop()?;
        map_item.set(key, value)?;
    }

    context.push(StackItem::Map(map_item))?;

    Ok(())
}

/// Implements the PACKSTRUCT operation.
fn pack_struct(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the count from the stack
    let count = context
        .pop()?
        .into_int()?
        .to_usize()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid struct size"))?;

    let mut items = Vec::with_capacity(count);
    for _ in 0..count {
        items.push(context.pop()?);
    }

    let structure = Struct::new(items, Some(context.reference_counter().clone()))?;
    context.push(StackItem::Struct(structure))?;

    Ok(())
}

/// Implements the PACK operation.
fn pack(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the count from the stack
    let count = context
        .pop()?
        .into_int()?
        .to_usize()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid array size"))?;

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
fn unpack(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

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
fn pick_item(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    let key = context.pop()?;
    let collection = context.pop()?;

    let result = match collection {
        StackItem::Array(array) => {
            let idx = normalize_index("VMArray", &key.get_integer()?, array.len())?;
            array
                .get(idx)
                .ok_or_else(|| VmError::invalid_operation_msg("Index out of range"))?
        }
        StackItem::Struct(structure) => {
            let idx = normalize_index("Struct", &key.get_integer()?, structure.len())?;
            structure.get(idx)?
        }
        StackItem::Map(map) => map.get(&key)?,
        StackItem::ByteString(bytes) => {
            let idx = normalize_index("PrimitiveType", &key.get_integer()?, bytes.len())?;
            pick_byte_sequence_item(neo_vm_rs::StackValue::ByteString(bytes.clone()), idx)?
        }
        // C# Neo VM PICKITEM on PrimitiveType reads the bytewise GetSpan()
        // representation. Use neo-vm-rs conversion rules so Boolean false
        // remains a one-byte span [0], matching C# Boolean.Memory.
        item @ (StackItem::Integer(_) | StackItem::Boolean(_)) => {
            let bytes = stack_value_byte_string_bytes(neo_vm_rs::StackValue::try_from(item)?)?;
            let idx = normalize_index("PrimitiveType", &key.get_integer()?, bytes.len())?;
            StackItem::from_int(i64::from(bytes[idx]))
        }
        StackItem::Buffer(buffer) => {
            let idx = normalize_index("Buffer", &key.get_integer()?, buffer.len())?;
            pick_byte_sequence_item(neo_vm_rs::StackValue::Buffer(buffer.data()), idx)?
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
fn set_item(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
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
            let idx = normalize_index("VMArray", &key.get_integer()?, array.len())?;
            array.set(idx, value)?;
        }
        StackItem::Struct(structure) => {
            if let Some(rc) = structure.reference_counter() {
                value.attach_reference_counter(&rc)?;
            }
            let idx = normalize_index("Struct", &key.get_integer()?, structure.len())?;
            structure.set(idx, value)?;
        }
        StackItem::Map(map) => {
            if let Some(rc) = map.reference_counter() {
                value.attach_reference_counter(&rc)?;
            }
            map.set(key, value)?;
        }
        StackItem::Buffer(buffer) => {
            let idx = normalize_index("Buffer", &key.get_integer()?, buffer.len())?;
            let primitive = value.as_primitive().map_err(|_| {
                VmError::invalid_operation_msg(format!(
                    "Only primitive type values can be set in Buffer in {:?}.",
                    instruction.opcode()
                ))
            })?;
            let byte = primitive.get_integer().map_err(|_| {
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
fn size(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

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
        item @ (StackItem::Integer(_) | StackItem::Boolean(_)) => {
            stack_value_byte_string_len(neo_vm_rs::StackValue::try_from(item)?)?
        }
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
