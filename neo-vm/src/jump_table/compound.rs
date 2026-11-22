//! Compound operations for the Neo Virtual Machine.
//!
//! This module provides the compound operation handlers for the Neo VM.

use crate::Instruction;
use crate::OpCode;
use crate::error::VmError;
use crate::error::VmResult;
use crate::execution_engine::ExecutionEngine;
use crate::jump_table::{JumpTable, register_jump_handlers};
use crate::reference_counter::{add_raw_stack_references, remove_raw_stack_references};
use crate::run_stats::StackItemType;
use crate::stack_item::{Array, Map, StackItem, Struct};
use num_traits::ToPrimitive;

mod build;
mod items;

use build::{
    new_array, new_array_t, new_array0, new_map, new_struct, new_struct0, pack, pack_map,
    pack_struct, unpack,
};
use items::{
    append, clear_items, has_key, keys, pick_item, pop_item, remove, reverse, size, values,
};

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

/// Formats a map key for a not-found diagnostic, like the C# `key is Integer
/// ? key.GetInteger().ToString(InvariantCulture) : key.ToString()` fallback.
fn key_text(key: &StackItem) -> String {
    match key.as_integer() {
        Ok(integer) => integer.to_string(),
        Err(_) => format!("{key:?}"),
    }
}

/// Turns a handler failure into a throw while keeping the instruction's
/// `RunStats` complete.
///
/// C# v3.11.2 `PICKITEM`/`SETITEM` call `ExecuteThrow(engine, message, out int
/// refsDelta)` inside the handler and add the unwind's reference delta to the
/// snapshots they already collected, so the post-instruction hook prices the
/// opcode with that delta included. `refs_before_throw` is the handler-side
/// portion of the opcode's reference delta: `r1 - r3` for PICKITEM and
/// `(r1 - r2) + (r3 - r2) + (r3 - r4)` for SETITEM. v3.11.2 always raises the
/// script-visible exception this way (no `CatchEngineExceptions` gate); when
/// no catch block exists, `ExecuteThrow` faults the engine with
/// `VMUnhandledException`.
fn throw_counted(
    engine: &mut ExecutionEngine,
    refs_before_throw: i64,
    message: String,
) -> VmResult<()> {
    let exception = StackItem::from_byte_string(message.into_bytes());
    match engine.execute_throw_counted(Some(exception)) {
        Ok(refs_delta) => {
            engine.collect_ref_delta(refs_before_throw + refs_delta);
            Ok(())
        }
        Err(err) => Err(err),
    }
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

/// Implements the SETITEM operation.
fn set_item(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let limits = *engine.limits();
    let r1 = engine.reference_count();

    // C# v3.11.2 pops the value without releasing its reference; for a struct
    // that reference moves onto the clone instead.
    let value = engine.pop_no_ref()?;
    let mut cloned_items = 0i64;
    let mut r2 = r1;
    let item = if let StackItem::Struct(structure) = &value {
        engine.reference_counter().remove_stack_reference(&value);
        r2 = engine.reference_count();
        let (clone, cloned) = structure.clone_with_limits_counted(&limits)?;
        cloned_items = cloned;
        let clone = StackItem::Struct(clone);
        engine.reference_counter().add_stack_reference(&clone, 1);
        clone
    } else {
        value.clone()
    };
    let r3 = engine.reference_count();
    let key = engine.pop()?;
    let collection = engine.pop()?;

    match collection {
        // C# v3.11.2: when the array is stack referenced the popped value's
        // reference transfers into it, so only the replaced item is released;
        // an unreferenced array releases the value's own reference.
        StackItem::Array(array) => {
            let index = key.as_integer()?;
            match index.to_usize() {
                Some(idx) if idx < array.len() => {
                    if array.is_stack_referenced() {
                        let replaced = array
                            .get(idx)
                            .ok_or_else(|| VmError::invalid_operation_msg("Index out of range"))?;
                        engine.reference_counter().remove_stack_reference(&replaced);
                    } else {
                        engine.reference_counter().remove_stack_reference(&item);
                    }
                    array.set(idx, item)?;
                }
                _ => {
                    engine.reference_counter().remove_stack_reference(&item);
                    let r4 = engine.reference_count();
                    throw_counted(
                        engine,
                        (r1 - r2) + (r3 - r2) + (r3 - r4),
                        format!(
                            "The index of VMArray is out of range, {index}/[0, {count}).",
                            count = array.len()
                        ),
                    )?;
                    engine.collect_cloned_items(cloned_items);
                    return Ok(());
                }
            }
        }
        StackItem::Struct(structure) => {
            let index = key.as_integer()?;
            match index.to_usize() {
                Some(idx) if idx < structure.len() => {
                    if structure.is_stack_referenced() {
                        let replaced = structure.get(idx)?;
                        engine.reference_counter().remove_stack_reference(&replaced);
                    } else {
                        engine.reference_counter().remove_stack_reference(&item);
                    }
                    structure.set(idx, item)?;
                }
                _ => {
                    engine.reference_counter().remove_stack_reference(&item);
                    let r4 = engine.reference_count();
                    throw_counted(
                        engine,
                        (r1 - r2) + (r3 - r2) + (r3 - r4),
                        format!(
                            "The index of VMArray is out of range, {index}/[0, {count}).",
                            count = structure.len()
                        ),
                    )?;
                    engine.collect_cloned_items(cloned_items);
                    return Ok(());
                }
            }
        }
        // C# v3.11.2: a referenced map releases the value it replaces or
        // claims the new key; an unreferenced map releases the value's own
        // reference. Either way the value's reference transfers into the map.
        StackItem::Map(map) => {
            if map.is_stack_referenced() {
                let reference_counter = engine.reference_counter();
                match map.try_get(&key)? {
                    Some(replaced) => reference_counter.remove_stack_reference(&replaced),
                    None => reference_counter.add_stack_reference(&key, 1),
                }
            } else {
                engine.reference_counter().remove_stack_reference(&item);
            }
            map.set(key, item)?;
        }
        StackItem::Buffer(buffer) => {
            // The value's reference is released before the range check.
            engine.reference_counter().remove_stack_reference(&item);
            let index = key.as_integer()?;
            match index.to_usize() {
                Some(idx) if idx < buffer.len() => {
                    let byte = item.as_integer().map_err(|_| {
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
                    let r4 = engine.reference_count();
                    throw_counted(
                        engine,
                        (r1 - r2) + (r3 - r2) + (r3 - r4),
                        format!(
                            "The index of Buffer is out of range, {index}/[0, {count}).",
                            count = buffer.len()
                        ),
                    )?;
                    engine.collect_cloned_items(cloned_items);
                    return Ok(());
                }
            }
        }
        _ => {
            return Err(VmError::invalid_type_simple(
                "Expected Array, Struct, Map, or Buffer",
            ));
        }
    }

    engine.collect_ref_delta((r1 - r2) + (r3 - r2) + (r3 - engine.reference_count()));
    engine.collect_cloned_items(cloned_items);

    Ok(())
}

/// Implements the SETITEM operation before neo-vm PR #606, the jump table
/// every height below `HF_Huyao` runs (C# `SetItem_Before606`).
///
/// The pre-606 behaviour releases the popped value's stack reference through
/// the regular pop, clones structs without touching the reference counter,
/// only adjusts references when the target collection is itself stack
/// referenced, and never feeds `RunStats` (the unwind's reference delta is
/// discarded). Pre-Gorgon heights ran the semantically equivalent
/// `SetItem_Before543`, so one variant covers every pre-Huyao height.
pub(crate) fn set_item_before_606(
    engine: &mut ExecutionEngine,
    instruction: &Instruction,
) -> VmResult<()> {
    let limits = *engine.limits();

    let popped = engine.pop()?;
    let value = if let StackItem::Struct(structure) = &popped {
        StackItem::Struct(structure.clone_with_limits(&limits)?)
    } else {
        popped
    };
    let key = engine.pop()?;
    let collection = engine.pop()?;

    match collection {
        StackItem::Array(array) => {
            let index = key.as_integer()?;
            match index.to_usize() {
                Some(idx) if idx < array.len() => {
                    if array.is_stack_referenced() {
                        let replaced = array
                            .get(idx)
                            .ok_or_else(|| VmError::invalid_operation_msg("Index out of range"))?;
                        engine.reference_counter().remove_stack_reference(&replaced);
                    }
                    array.set(idx, value)?;
                    // C# re-evaluates `IsStackReferenced` after storing: the
                    // removal above drops the array's own last stack
                    // reference when it replaced itself.
                    if array.is_stack_referenced() {
                        let stored = array
                            .get(idx)
                            .ok_or_else(|| VmError::invalid_operation_msg("Index out of range"))?;
                        engine.reference_counter().add_stack_reference(&stored, 1);
                    }
                }
                _ => {
                    let exception = StackItem::from_byte_string(
                        format!(
                            "The index of VMArray is out of range, {index}/[0, {count}).",
                            count = array.len()
                        )
                        .into_bytes(),
                    );
                    // C# discards the unwind's reference delta (`out int _`).
                    return engine.execute_throw_counted(Some(exception)).map(|_| ());
                }
            }
        }
        StackItem::Struct(structure) => {
            let index = key.as_integer()?;
            match index.to_usize() {
                Some(idx) if idx < structure.len() => {
                    if structure.is_stack_referenced() {
                        let replaced = structure.get(idx)?;
                        engine.reference_counter().remove_stack_reference(&replaced);
                    }
                    structure.set(idx, value)?;
                    if structure.is_stack_referenced() {
                        let stored = structure.get(idx)?;
                        engine.reference_counter().add_stack_reference(&stored, 1);
                    }
                }
                _ => {
                    let exception = StackItem::from_byte_string(
                        format!(
                            "The index of VMArray is out of range, {index}/[0, {count}).",
                            count = structure.len()
                        )
                        .into_bytes(),
                    );
                    return engine.execute_throw_counted(Some(exception)).map(|_| ());
                }
            }
        }
        StackItem::Map(map) => {
            if map.is_stack_referenced() {
                let reference_counter = engine.reference_counter();
                match map.try_get(&key)? {
                    Some(replaced) => reference_counter.remove_stack_reference(&replaced),
                    None => reference_counter.add_stack_reference(&key, 1),
                }
                reference_counter.add_stack_reference(&value, 1);
            }
            map.set(key, value)?;
        }
        StackItem::Buffer(buffer) => {
            let index = key.as_integer()?;
            match index.to_usize() {
                Some(idx) if idx < buffer.len() => {
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
                    let exception = StackItem::from_byte_string(
                        format!(
                            "The index of Buffer is out of range, {index}/[0, {count}).",
                            count = buffer.len()
                        )
                        .into_bytes(),
                    );
                    return engine.execute_throw_counted(Some(exception)).map(|_| ());
                }
            }
        }
        _ => {
            return Err(VmError::invalid_type_simple(
                "Expected Array, Struct, Map, or Buffer",
            ));
        }
    }

    Ok(())
}
