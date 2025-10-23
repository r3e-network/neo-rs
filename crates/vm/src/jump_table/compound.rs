//! Compound operations for the Neo Virtual Machine.
//!
//! This module provides the compound operation handlers for the Neo VM.

use crate::error::VmError;
use crate::error::VmResult;
use crate::execution_engine::ExecutionEngine;
use crate::instruction::Instruction;
use crate::jump_table::JumpTable;
use crate::op_code::OpCode;
use crate::stack_item::primitive_type::PrimitiveTypeExt;
use crate::stack_item::{Array, Map, StackItem, Struct};
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use std::collections::BTreeMap;

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

    let rc = context.reference_counter().clone();
    let array = Array::new(Vec::new(), Some(rc));
    context.push(StackItem::Array(array))?;

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
        .as_int()?
        .to_usize()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid array size"))?;

    let mut items = Vec::with_capacity(count);
    for _ in 0..count {
        items.push(StackItem::Null);
    }

    let array = Array::new(items, Some(context.reference_counter().clone()));
    context.push(StackItem::Array(array))?;

    Ok(())
}

/// Implements the NewarrayT operation.
fn new_array_t(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the count from the stack
    let count = context
        .pop()?
        .as_int()?
        .to_usize()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid array size"))?;

    // Get the type from the instruction
    let type_byte = instruction
        .operand()
        .first()
        .copied()
        .ok_or_else(|| VmError::invalid_instruction_msg("Missing type operand"))?;

    let mut items = Vec::with_capacity(count);
    for _ in 0..count {
        // Create a default value based on the type
        let default_value = match type_byte {
            0x00 => StackItem::Boolean(false),
            0x01 => StackItem::Integer(BigInt::from(0)),
            0x02 => StackItem::from_byte_string(Vec::<u8>::new()),
            0x03 => StackItem::from_buffer(Vec::<u8>::new()),
            0x04 => StackItem::Array(Array::new(
                Vec::<StackItem>::new(),
                Some(context.reference_counter().clone()),
            )),
            0x05 => StackItem::Struct(Struct::new(
                Vec::<StackItem>::new(),
                Some(context.reference_counter().clone()),
            )),
            0x06 => StackItem::Map(Map::new(
                BTreeMap::new(),
                Some(context.reference_counter().clone()),
            )),
            _ => {
                return Err(VmError::invalid_instruction_msg(format!(
                    "Invalid type: {type_byte}"
                )));
            }
        };

        items.push(default_value);
    }

    let array = Array::new(items, Some(context.reference_counter().clone()));
    context.push(StackItem::Array(array))?;

    Ok(())
}

/// Implements the NEWSTRUCT0 operation.
fn new_struct0(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    let structure = Struct::new(Vec::new(), Some(context.reference_counter().clone()));
    context.push(StackItem::Struct(structure))?;

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
        .as_int()?
        .to_usize()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid struct size"))?;

    let mut items = Vec::with_capacity(count);
    for _ in 0..count {
        items.push(StackItem::Null);
    }

    let structure = Struct::new(items, Some(context.reference_counter().clone()));
    context.push(StackItem::Struct(structure))?;

    Ok(())
}

/// Implements the NEWMAP operation.
fn new_map(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    let map = Map::new(BTreeMap::new(), Some(context.reference_counter().clone()));
    context.push(StackItem::Map(map))?;

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
        StackItem::Array(mut array) => {
            array.push(item)?;
            // APPEND pops both operands and does not push the array back (Pop 2, Push 0).
        }
        StackItem::Struct(mut structure) => {
            structure.push(item)?;
        }
        _ => return Err(VmError::invalid_type_simple("Expected Array or Struct")),
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
        StackItem::Array(mut array) => {
            array.items_mut().reverse();
            context.push(StackItem::Array(array))?;
        }
        StackItem::Struct(mut structure) => {
            structure.items_mut().reverse();
            context.push(StackItem::Struct(structure))?;
        }
        _ => return Err(VmError::invalid_type_simple("Expected Array or Struct")),
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
        StackItem::Array(mut array) => {
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
            context.push(StackItem::Array(array))?;
        }
        StackItem::Struct(mut structure) => {
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
            context.push(StackItem::Struct(structure))?;
        }
        StackItem::Map(mut map) => {
            let _ = map.remove(&key)?;
            context.push(StackItem::Map(map))?;
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
        StackItem::Array(mut array) => {
            array.clear()?;
            context.push(StackItem::Array(array))?;
        }
        StackItem::Struct(mut structure) => {
            structure.clear()?;
            context.push(StackItem::Struct(structure))?;
        }
        StackItem::Map(mut map) => {
            map.clear()?;
            context.push(StackItem::Map(map))?;
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
        StackItem::Array(mut array) => {
            let popped_item = array.pop()?;
            context.push(StackItem::Array(array))?;
            context.push(popped_item)?;
        }
        StackItem::Struct(mut structure) => {
            let popped_item = structure.pop()?;
            context.push(StackItem::Struct(structure))?;
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

    let result = match collection {
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
        _ => {
            return Err(VmError::invalid_type_simple(
                "Expected Array, Struct, or Map",
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
            let keys: Vec<StackItem> = map.iter().map(|(k, _)| k.clone()).collect();
            let array = Array::new(keys, Some(context.reference_counter().clone()));
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
            let values: Vec<StackItem> = map.iter().map(|(_, v)| v.clone()).collect();
            let array = Array::new(values, Some(context.reference_counter().clone()));
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
        .as_int()?
        .to_usize()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid map size"))?;

    let mut map_item = Map::new(BTreeMap::new(), Some(context.reference_counter().clone()));

    for _ in 0..count {
        let value = context.pop()?;
        let key = context.pop()?;
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
        .as_int()?
        .to_usize()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid struct size"))?;

    let mut items = Vec::with_capacity(count);
    for _ in 0..count {
        items.push(context.pop()?);
    }

    items.reverse();

    let structure = Struct::new(items, Some(context.reference_counter().clone()));
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
        .as_int()?
        .to_usize()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid array size"))?;

    // Create a new array
    let mut items = Vec::with_capacity(count);
    for _ in 0..count {
        items.push(context.pop()?);
    }

    items.reverse();

    let array = Array::new(items, Some(context.reference_counter().clone()));
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

    // Unpack the array
    match array {
        StackItem::Array(array) => {
            for item in array.iter() {
                context.push(item.clone())?;
            }
            context.push(StackItem::from_int(array.len()))?;
        }
        StackItem::Struct(structure) => {
            for item in structure.iter() {
                context.push(item.clone())?;
            }
            context.push(StackItem::from_int(structure.len()))?;
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
            array.items()[idx].clone()
        }
        StackItem::Struct(structure) => {
            let idx = normalize_index("Struct", &key.get_integer()?, structure.len())?;
            structure.items()[idx].clone()
        }
        StackItem::Map(map) => map.get(&key)?.clone(),
        StackItem::ByteString(bytes) => {
            let idx = normalize_index("PrimitiveType", &key.get_integer()?, bytes.len())?;
            StackItem::from_int(bytes[idx] as i64)
        }
        StackItem::Buffer(buffer) => {
            let idx = normalize_index("Buffer", &key.get_integer()?, buffer.len())?;
            StackItem::from_int(buffer.data()[idx] as i64)
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
        StackItem::Array(mut array) => {
            let idx = normalize_index("VMArray", &key.get_integer()?, array.len())?;
            array.set(idx, value)?;
            context.push(StackItem::Array(array))?;
        }
        StackItem::Struct(mut structure) => {
            let idx = normalize_index("Struct", &key.get_integer()?, structure.len())?;
            structure.set(idx, value)?;
            context.push(StackItem::Struct(structure))?;
        }
        StackItem::Map(mut map) => {
            map.set(key, value)?;
            context.push(StackItem::Map(map))?;
        }
        StackItem::Buffer(mut buffer) => {
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
            if byte < i8::MIN as i32 || byte > u8::MAX as i32 {
                return Err(VmError::invalid_operation_msg(format!(
                    "Overflow in {:?}, {byte} is not a byte type.",
                    instruction.opcode()
                )));
            }
            buffer.set(idx, byte as u8)?;
            context.push(StackItem::Buffer(buffer))?;
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

    // Get the size of the collection
    let size = match collection {
        StackItem::Array(array) => array.len(),
        StackItem::Struct(structure) => structure.len(),
        StackItem::Map(map) => map.len(),
        StackItem::ByteString(data) => data.len(),
        StackItem::Buffer(data) => data.len(),
        _ => {
            return Err(VmError::invalid_type_simple(
                "Expected Array, Struct, Map, ByteString, or Buffer",
            ));
        }
    };

    // Push the size onto the stack
    context.push(StackItem::from_int(size))?;

    Ok(())
}
