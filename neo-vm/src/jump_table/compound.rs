//! Compound operations for the Neo Virtual Machine.
//!
//! This module provides the compound operation handlers for the Neo VM.

use crate::Instruction;
use crate::OpCode;
use crate::StackItemType;
use crate::error::VmError;
use crate::error::VmResult;
use crate::execution_engine::ExecutionEngine;
use crate::jump_table::{JumpTable, register_jump_handlers};
use crate::stack_item::{Array, Map, StackItem, Struct};
use num_bigint::BigInt;
use num_traits::ToPrimitive;

/// What the reference counter needs to know about `item`. A compound item
/// shares its storage with the clone; any other item only adds one to the
/// count, so its payload is not copied.
fn counted(item: &StackItem) -> StackItem {
    match item {
        StackItem::Array(_) | StackItem::Struct(_) | StackItem::Map(_) => item.clone(),
        _ => StackItem::Null,
    }
}

fn collection_stack_item(value: Result<crate::StackValue, String>) -> VmResult<StackItem> {
    StackItem::try_from(value.map_err(VmError::invalid_operation_msg)?)
}

fn normalize_index(type_name: &str, index: &BigInt, length: usize) -> VmResult<usize> {
    if let Some(idx) = index.to_usize()
        && idx < length
    {
        return Ok(idx);
    }

    Err(VmError::catchable_exception_msg(format!(
        "The index of {type_name} is out of range, {index}/[0, {length})."
    )))
}

fn byte_sequence_key_value(key: &StackItem) -> VmResult<crate::StackValue> {
    let index = key
        .as_int()?
        .to_i64()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid index"))?;
    Ok(crate::StackValue::Integer(index))
}

fn byte_sequence_has_key(
    value: crate::StackValue,
    key: &StackItem,
    pre_gorgon: bool,
) -> VmResult<bool> {
    let key = byte_sequence_key_value(key)?;
    let result = if pre_gorgon {
        crate::semantics::collections::has_key_pre_gorgon(&value, &key)
    } else {
        crate::semantics::collections::has_key(&value, &key)
    };
    result.map_err(VmError::invalid_operation_msg)
}

/// Post-Gorgon HASKEY rejects indices at/above `MaxItemSize`; pre-Gorgon only
/// rejects negative values (Neo PR #543). Arrays/structs must follow the same
/// rule as ByteString/Buffer.
fn has_key_collection_index(key: &StackItem, pre_gorgon: bool, type_name: &str) -> VmResult<usize> {
    let index = key
        .as_int()?
        .to_i64()
        .ok_or_else(|| VmError::invalid_operation_msg(format!("Invalid {type_name} index")))?;
    if index < 0 {
        return Err(VmError::invalid_operation_msg(format!(
            "HASKEY: negative {type_name} index"
        )));
    }
    if !pre_gorgon && index >= crate::MAX_ITEM_SIZE as i64 {
        return Err(VmError::invalid_operation_msg(
            "HASKEY: index exceeds MaxItemSize",
        ));
    }
    usize::try_from(index)
        .map_err(|_| VmError::invalid_operation_msg(format!("Invalid {type_name} index")))
}

fn pick_byte_sequence_item(value: crate::StackValue, index: usize) -> VmResult<StackItem> {
    let key = crate::StackValue::Integer(
        i64::try_from(index).map_err(|_| VmError::invalid_operation_msg("Invalid index"))?,
    );
    StackItem::try_from(
        crate::semantics::collections::pick_item(&value, &key)
            .map_err(VmError::invalid_operation_msg)?,
    )
}

/// Registers the compound operation handlers.
pub fn register_handlers(jump_table: &mut JumpTable) {
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
fn new_array0(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let max_items = engine.limits().max_stack_size as usize;
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    let array = collection_stack_item(crate::semantics::collections::new_array(0, max_items))?;
    context.push(array)?;

    Ok(())
}

/// Implements the NEWARRAY operation.
fn new_array(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let max_items = engine.limits().max_stack_size as usize;
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

    let array = collection_stack_item(crate::semantics::collections::new_array(count, max_items))?;
    context.push(array)?;

    Ok(())
}

/// Implements the `NewarrayT` operation.
fn new_array_t(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let max_items = engine.limits().max_stack_size as usize;
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

    let array = collection_stack_item(crate::semantics::collections::new_array_t(
        count, type_byte, max_items,
    ))?;
    context.push(array)?;

    Ok(())
}

/// Implements the NEWSTRUCT0 operation.
fn new_struct0(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let max_items = engine.limits().max_stack_size as usize;
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    let structure = collection_stack_item(crate::semantics::collections::new_struct(0, max_items))?;
    context.push(structure)?;

    Ok(())
}

/// Implements the NEWSTRUCT operation.
fn new_struct(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let max_items = engine.limits().max_stack_size as usize;
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

    let structure =
        collection_stack_item(crate::semantics::collections::new_struct(count, max_items))?;
    context.push(structure)?;

    Ok(())
}

/// Implements the NEWMAP operation.
fn new_map(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    let map_value = crate::semantics::collections::pack_map(Vec::new());
    let map = collection_stack_item(Ok(map_value))?;
    context.push(map)?;

    Ok(())
}

/// Implements the APPEND operation.
fn append(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let limits = *engine.limits();
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    let mut item = context.pop()?;
    let collection = context.pop()?;
    if !matches!(collection, StackItem::Array(_) | StackItem::Struct(_)) {
        return Err(VmError::invalid_type_simple(
            "Expected Array, Struct, or Map",
        ));
    }

    if let StackItem::Struct(structure) = item {
        item = StackItem::Struct(structure.clone_with_limits(&limits)?);
    }

    // C# v3.10.1: `array.Add(newItem)`, then, if the array is stack
    // referenced, `AddStackReference(newItem)`.
    // APPEND pops both operands and does not push the array back (Pop 2, Push 0).
    let added = counted(&item);
    let referenced = match collection {
        StackItem::Array(array) => {
            array.push(item)?;
            array.is_stack_referenced()
        }
        StackItem::Struct(structure) => {
            structure.push(item)?;
            structure.is_stack_referenced()
        }
        _ => false,
    };
    if referenced {
        context.reference_counter().add_stack_reference(&added, 1);
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
            // C# v3.10.1: `RemoveAt(i)` first, then release the removed item
            // if the array is stack referenced.
            let removed = array.remove(index)?;
            if array.is_stack_referenced() {
                context.reference_counter().remove_stack_reference(&removed);
            }
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
            let removed = structure.remove(index)?;
            if structure.is_stack_referenced() {
                context.reference_counter().remove_stack_reference(&removed);
            }
        }
        StackItem::Map(map) => {
            // C# v3.10.1: `map.Remove(key)` first; a removed entry releases
            // its key and its value if the map is stack referenced.
            if let Some(removed) = map.remove_entry(&key)?
                && map.is_stack_referenced()
            {
                let reference_counter = context.reference_counter();
                reference_counter.remove_stack_reference(&key);
                reference_counter.remove_stack_reference(&removed);
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
fn clear_items(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the collection from the stack
    let collection = context.pop()?;

    // Clear the collection. C# v3.10.1: take the sub items, `Clear()`, then
    // release every sub item if the collection is stack referenced.
    let (sub_items, referenced) = match collection {
        StackItem::Array(array) => {
            let sub_items = array.sub_items();
            array.clear()?;
            (sub_items, array.is_stack_referenced())
        }
        StackItem::Struct(structure) => {
            let sub_items = structure.sub_items();
            structure.clear()?;
            (sub_items, structure.is_stack_referenced())
        }
        StackItem::Map(map) => {
            let sub_items = map.sub_items();
            map.clear()?;
            (sub_items, map.is_stack_referenced())
        }
        _ => {
            return Err(VmError::invalid_type_simple(
                "Expected Array, Struct, or Map",
            ));
        }
    };
    if referenced {
        context.reference_counter().remove_sub_items(sub_items);
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

    // C# v3.10.1: push the last item, `RemoveAt(index)`, then release the
    // removed item if the collection is stack referenced.
    match collection {
        StackItem::Array(array) => {
            let last = array
                .with_items(|items| items.last().cloned())
                .ok_or_else(|| VmError::invalid_operation_msg("Array is empty"))?;
            context.push(last)?;
            let removed = array.pop()?;
            if array.is_stack_referenced() {
                context.reference_counter().remove_stack_reference(&removed);
            }
        }
        StackItem::Struct(structure) => {
            let last = structure
                .with_items(|items| items.last().cloned())
                .ok_or_else(|| VmError::invalid_operation_msg("Struct is empty"))?;
            context.push(last)?;
            let removed = structure.pop()?;
            if structure.is_stack_referenced() {
                context.reference_counter().remove_stack_reference(&removed);
            }
        }
        _ => return Err(VmError::invalid_type_simple("Expected Array or Struct")),
    }

    Ok(())
}

/// Implements the HASKEY operation.
fn has_key(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let pre_gorgon = engine.limits().pre_gorgon_collection_semantics;
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the key and collection from the stack
    let key = context.pop()?;
    let collection = context.pop()?;

    let result = match &collection {
        StackItem::Array(array) => {
            let index = has_key_collection_index(&key, pre_gorgon, "array")?;
            index < array.len()
        }
        StackItem::Struct(structure) => {
            let index = has_key_collection_index(&key, pre_gorgon, "struct")?;
            index < structure.len()
        }
        StackItem::Map(map) => map.contains_key(&key)?,
        StackItem::ByteString(bytes) => byte_sequence_has_key(
            crate::StackValue::ByteString(bytes.clone()),
            &key,
            pre_gorgon,
        )?,
        StackItem::Buffer(buffer) => {
            byte_sequence_has_key(crate::StackValue::Buffer(buffer.data()), &key, pre_gorgon)?
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
            context.push(StackItem::Array(Array::new(keys)))?;
        }
        _ => return Err(VmError::invalid_type_simple("Expected Map")),
    }

    Ok(())
}

/// Implements the VALUES operation.
fn values(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let limits = *engine.limits();
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    let item = context.pop()?;
    let values: Vec<StackItem> = match item {
        // C# `VMArray` includes `Struct` (Struct : Array).
        StackItem::Array(array) => array.items(),
        StackItem::Struct(structure) => structure.items(),
        StackItem::Map(map) => {
            map.with_items(|items| items.iter().map(|(_, value)| value.clone()).collect())
        }
        _ => {
            return Err(VmError::invalid_type_simple(
                "VALUES expects an Array, Struct, or Map",
            ));
        }
    };

    // C# clones nested Struct values (which are mutable) with the engine
    // limits; other item kinds are copied by reference.
    let mut values_out = Vec::with_capacity(values.len());
    for value in values {
        match value {
            StackItem::Struct(structure) => {
                values_out.push(StackItem::Struct(structure.clone_with_limits(&limits)?));
            }
            other => values_out.push(other),
        }
    }

    context.push(StackItem::Array(Array::new(values_out)))?;
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
    let required = count
        .checked_mul(2)
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid map size"))?;
    let stack_len = context.evaluation_stack().len();
    if required > stack_len {
        return Err(VmError::stack_underflow_msg(required, stack_len));
    }

    let map_item = Map::new(crate::VmOrderedDictionary::new());

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
    let stack_len = context.evaluation_stack().len();
    if count > stack_len {
        return Err(VmError::stack_underflow_msg(count, stack_len));
    }

    let mut items = Vec::with_capacity(count);
    for _ in 0..count {
        items.push(context.pop()?);
    }

    context.push(StackItem::Struct(Struct::new(items)))?;

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
    let stack_len = context.evaluation_stack().len();
    if count > stack_len {
        return Err(VmError::stack_underflow_msg(count, stack_len));
    }

    // Create a new array
    let mut items = Vec::with_capacity(count);
    for _ in 0..count {
        items.push(context.pop()?);
    }

    context.push(StackItem::Array(Array::new(items)))?;

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
            pick_byte_sequence_item(crate::StackValue::ByteString(bytes.clone()), idx)?
        }
        // C# Neo VM PICKITEM on PrimitiveType reads the bytewise GetSpan()
        // representation. Use neo-vm-rs conversion rules so Boolean false
        // remains a one-byte span [0], matching C# Boolean.Memory.
        item @ (StackItem::Integer(_) | StackItem::Boolean(_)) => {
            let value = crate::StackValue::try_from(item)?;
            let len = crate::semantics::collections::size(&value)
                .map_err(VmError::invalid_operation_msg)? as usize;
            let idx = normalize_index("PrimitiveType", &key.as_integer()?, len)?;
            pick_byte_sequence_item(value, idx)?
        }
        StackItem::Buffer(buffer) => {
            let idx = normalize_index("Buffer", &key.as_integer()?, buffer.len())?;
            pick_byte_sequence_item(crate::StackValue::Buffer(buffer.data()), idx)?
        }
        other => {
            return Err(VmError::invalid_type_simple(format!(
                "PICKITEM: unsupported {:?}",
                other.stack_item_type()
            )));
        }
    };

    context.push(result)?;

    Ok(())
}

/// Implements the SETITEM operation.
fn set_item(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let limits = *engine.limits();
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // C# v3.10.1 clones a Struct value right after popping it, before the
    // key and the collection are popped.
    let mut value = context.pop()?;
    if let StackItem::Struct(structure) = value {
        value = StackItem::Struct(structure.clone_with_limits(&limits)?);
    }
    let key = context.pop()?;
    let collection = context.pop()?;

    match collection {
        // C# v3.10.1: release the replaced item if the array is stack
        // referenced, assign, then add the new item if the array is stack
        // referenced.
        StackItem::Array(array) => {
            let idx = normalize_index("VMArray", &key.as_integer()?, array.len())?;
            let added = counted(&value);
            if array.is_stack_referenced()
                && let Some(replaced) = array.with_items(|items| items.get(idx).map(counted))
            {
                context
                    .reference_counter()
                    .remove_stack_reference(&replaced);
            }
            array.set(idx, value)?;
            if array.is_stack_referenced() {
                context.reference_counter().add_stack_reference(&added, 1);
            }
        }
        StackItem::Struct(structure) => {
            let idx = normalize_index("Struct", &key.as_integer()?, structure.len())?;
            let added = counted(&value);
            if structure.is_stack_referenced()
                && let Some(replaced) = structure.with_items(|items| items.get(idx).map(counted))
            {
                context
                    .reference_counter()
                    .remove_stack_reference(&replaced);
            }
            structure.set(idx, value)?;
            if structure.is_stack_referenced() {
                context.reference_counter().add_stack_reference(&added, 1);
            }
        }
        // C# v3.10.1: if the map is stack referenced, a new key gets a
        // reference and an existing key releases its old value, then the new
        // value gets a reference; the assignment comes last.
        StackItem::Map(map) => {
            if map.is_stack_referenced() {
                let reference_counter = context.reference_counter();
                match map.try_get(&key)? {
                    None => reference_counter.add_stack_reference(&key, 1),
                    Some(replaced) => reference_counter.remove_stack_reference(&replaced),
                }
                reference_counter.add_stack_reference(&value, 1);
            }
            map.set(key, value)?;
        }
        StackItem::Buffer(buffer) => {
            let idx = normalize_index("Buffer", &key.as_integer()?, buffer.len())?;
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
            let value = crate::StackValue::try_from(item)?;
            usize::try_from(
                crate::semantics::collections::size(&value)
                    .map_err(VmError::invalid_operation_msg)?,
            )
            .map_err(|_| VmError::invalid_operation_msg("Invalid primitive size"))?
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
