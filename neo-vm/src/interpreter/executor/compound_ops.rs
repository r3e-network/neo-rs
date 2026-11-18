use super::super::api::InterpreterSemantics;
use super::super::helpers::*;
use super::super::opcodes::*;
use super::super::runtime_types::{
    CompoundIds, NESTING_DEPTH_ERROR, StackValue, ensure_nestable, find_affected_indices,
    propagate_update,
};
use super::super::state::{MAX_STACK_SIZE, PendingException, TryStack, remember_consumed_mutation};
use super::control::Dispatch;
use crate::{
    new_array_default_value_for_neovm_type_tag, semantics::collections as collection_rules,
};
use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};

#[inline]
#[allow(clippy::too_many_arguments)]
fn commit_collection_update(
    updated: &StackValue,
    stack: &mut [StackValue],
    locals: &mut [StackValue],
    args: &mut [StackValue],
    static_fields: &mut [StackValue],
    consumed_mutations: &mut Vec<StackValue>,
    affected_stack_indices: Option<&[usize]>,
) -> Result<(), String> {
    remember_consumed_mutation(consumed_mutations, updated);
    propagate_update(
        updated,
        stack,
        locals,
        args,
        static_fields,
        affected_stack_indices,
    )
}

/// Rejects a PACK-family operand run when wrapping it in one more compound
/// would nest past the limit. Runs before anything is popped, so the value
/// that would be too deep is never built.
fn ensure_operands_nestable(operands: &[StackValue]) -> Result<(), String> {
    if operands
        .iter()
        .any(|operand| ensure_nestable(operand).is_err())
    {
        return Err(NESTING_DEPTH_ERROR.to_string());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
#[inline]
pub(super) fn execute(
    opcode: u8,
    script: &[u8],
    ip_ref: &mut usize,
    stack: &mut Vec<StackValue>,
    ids: &mut CompoundIds,
    locals: &mut [StackValue],
    args: &mut [StackValue],
    static_fields: &mut [StackValue],
    consumed_mutations: &mut Vec<StackValue>,
    try_frames: &mut TryStack,
    pending_error: &mut Option<PendingException>,
    semantics: InterpreterSemantics,
) -> Result<Dispatch, String> {
    let mut ip = *ip_ref;
    macro_rules! finish {
        ($dispatch:expr) => {{
            *ip_ref = ip;
            return Ok($dispatch);
        }};
    }

    match opcode {
        // COMPOUND TYPE OPERATIONS (0xbe-0xd3)
        // =============================================================================
        PACKMAP => {
            let count = collection_rules::non_negative_count(
                pop_integer(stack)?,
                "negative count for PACKMAP",
            )?;
            if stack.len() < count.saturating_mul(2) {
                return Err("stack underflow for PACKMAP".to_string());
            }
            ensure_operands_nestable(&stack[stack.len() - count * 2..])?;

            let mut pairs = Vec::with_capacity(count);
            for _ in 0..count {
                let key = pop_item(stack)?;
                let value = pop_item(stack)?;
                pairs.push((key, value));
            }
            stack.push(ids.map(pairs));
        }
        PACKSTRUCT => {
            let count = collection_rules::non_negative_count(
                pop_integer(stack)?,
                "negative count for PACKSTRUCT",
            )?;
            if stack.len() < count {
                return Err("stack underflow for PACKSTRUCT".to_string());
            }
            ensure_operands_nestable(&stack[stack.len() - count..])?;

            let mut items = Vec::with_capacity(count);
            for _ in 0..count {
                items.push(pop_item(stack)?);
            }
            stack.push(ids.r#struct(items));
        }
        PACK => {
            let count = collection_rules::non_negative_count(
                pop_integer(stack)?,
                "negative count for PACK",
            )?;
            if stack.len() < count {
                return Err("stack underflow for PACK".to_string());
            }
            ensure_operands_nestable(&stack[stack.len() - count..])?;

            let mut items = Vec::with_capacity(count);
            for _ in 0..count {
                items.push(pop_item(stack)?);
            }
            stack.push(ids.array(items));
        }
        UNPACK => {
            let mut item = pop_item(stack)?;
            match &mut item {
                StackValue::Array(_, items) | StackValue::Struct(_, items) => {
                    let items = core::mem::take(items);
                    let count = items.len() as i64;
                    for item in items.into_iter().rev() {
                        stack.push(item);
                    }
                    stack.push(StackValue::Integer(count));
                }
                StackValue::Map(_, items) => {
                    let items = core::mem::take(items);
                    let count = items.len() as i64;
                    for (key, value) in items.into_iter().rev() {
                        stack.push(value);
                        stack.push(key);
                    }
                    stack.push(StackValue::Integer(count));
                }
                _ => return Err("UNPACK expects an array, struct, or map".to_string()),
            }
        }
        NEWARRAY0 => {
            stack.push(ids.array(Vec::new()));
        }
        NEWARRAY => {
            let count = collection_rules::non_negative_count(
                pop_integer(stack)?,
                "negative count for NEWARRAY",
            )?;
            if count > MAX_STACK_SIZE {
                return Err("NEWARRAY count exceeds maximum stack size".to_string());
            }
            stack.push(ids.array(vec![StackValue::Null; count]));
        }
        NEWARRAY_T => {
            if ip + 2 > script.len() {
                return Err("truncated NEWARRAY_T type".to_string());
            }
            let count = collection_rules::non_negative_count(
                pop_integer(stack)?,
                "negative count for NEWARRAY_T",
            )?;
            if count > MAX_STACK_SIZE {
                return Err("NEWARRAY_T count exceeds maximum stack size".to_string());
            }
            let kind = script[ip + 1];
            let default_value = ids.import_abi(new_array_default_value_for_neovm_type_tag(kind));
            stack.push(ids.array(vec![default_value; count]));
            ip += 2;
            finish!(Dispatch::Continue);
        }
        NEWSTRUCT0 => {
            stack.push(ids.r#struct(Vec::new()));
        }
        NEWSTRUCT => {
            let count = collection_rules::non_negative_count(
                pop_integer(stack)?,
                "negative count for NEWSTRUCT",
            )?;
            if count > MAX_STACK_SIZE {
                return Err("NEWSTRUCT count exceeds maximum stack size".to_string());
            }
            stack.push(ids.r#struct(vec![StackValue::Null; count]));
        }
        NEWMAP => {
            stack.push(ids.map(Vec::new()));
        }
        SIZE => {
            let item = pop_item(stack)?;
            let size = collection_rules::size(&to_abi_value(&item))
                .map_err(|_| "SIZE expects a collection".to_string())?;
            stack.push(StackValue::Integer(size));
        }
        HASKEY => {
            let key = pop_item(stack)?;
            let item = pop_item(stack)?;
            let has_key = if semantics.pre_gorgon_collection_semantics {
                collection_rules::has_key_pre_gorgon(&to_abi_value(&item), &to_abi_value(&key))
            } else {
                collection_rules::has_key(&to_abi_value(&item), &to_abi_value(&key))
            }
            .map_err(|_| "HASKEY expects an array, buffer, or map".to_string())?;
            stack.push(StackValue::Boolean(has_key));
        }
        KEYS => {
            let mut item = pop_item(stack)?;
            match &mut item {
                StackValue::Map(_, items) => {
                    let items = core::mem::take(items);
                    let len = items.len();
                    let keys: Vec<_> = {
                        let mut v = Vec::with_capacity(len);
                        v.extend(items.into_iter().map(|(key, _)| key));
                        v
                    };
                    stack.push(ids.array(keys));
                }
                _ => return Err("KEYS expects a map".to_string()),
            }
        }
        VALUES => {
            let mut item = pop_item(stack)?;
            let values = match &mut item {
                StackValue::Map(_, items) => core::mem::take(items)
                    .into_iter()
                    .map(|(_, value)| value)
                    .collect::<Vec<_>>(),
                StackValue::Array(_, items) | StackValue::Struct(_, items) => {
                    core::mem::take(items)
                }
                _ => return Err("VALUES expects a map, array, or struct".to_string()),
            };
            // C# v3.10.1 clones the Struct values it collects, because they are
            // mutable value types; every other item is shared.
            let values = values
                .into_iter()
                .map(|value| ids.clone_struct_for_storage(&value))
                .collect();
            stack.push(ids.array(values));
        }
        APPEND => {
            let value = pop_item(stack)?;
            let mut item = pop_item(stack)?;
            ensure_nestable(&value)?;
            match &mut item {
                StackValue::Array(id, items) => {
                    let id = *id;
                    items.push(ids.clone_for_mutation(&value));
                    let items = core::mem::take(items);
                    let updated = StackValue::Array(id, items);
                    let affected = find_affected_indices(id, stack);
                    commit_collection_update(
                        &updated,
                        stack,
                        locals,
                        args,
                        static_fields,
                        consumed_mutations,
                        Some(&affected),
                    )?;
                }
                StackValue::Struct(id, items) => {
                    let id = *id;
                    items.push(ids.clone_for_mutation(&value));
                    let items = core::mem::take(items);
                    let updated = StackValue::Struct(id, items);
                    let affected = find_affected_indices(id, stack);
                    commit_collection_update(
                        &updated,
                        stack,
                        locals,
                        args,
                        static_fields,
                        consumed_mutations,
                        Some(&affected),
                    )?;
                }
                _ => return Err("APPEND expects an array or struct".to_string()),
            }
        }
        PICKITEM => {
            let pick_result = (|| -> Result<(), String> {
                let key_or_index = pop_item(stack)?;
                let mut item = pop_item(stack)?;
                match &mut item {
                    StackValue::Map(_, items) => {
                        // Map key can be any primitive type
                        let index = collection_rules::map_entry_index_by(
                            &*items,
                            &key_or_index,
                            primitive_key_equals,
                            validate_map_key,
                        )?
                        .ok_or_else(|| "key not found for PICKITEM".to_string())?;
                        stack.push(items[index].1.clone());
                    }
                    StackValue::ByteString(_)
                    | StackValue::Buffer(_, _)
                    | StackValue::Integer(_)
                    | StackValue::BigInteger(_)
                    | StackValue::Boolean(_) => {
                        let value = collection_rules::pick_item(
                            &to_abi_value(&item),
                            &to_abi_value(&key_or_index),
                        )
                        .map_err(|_| "index out of range for PICKITEM".to_string())?;
                        stack.push(ids.import_abi(value));
                    }
                    _ => {
                        // Array and Struct preserve interpreter object identity when
                        // a compound value is picked from them.
                        let index = match &key_or_index {
                            StackValue::Integer(v) if *v >= 0 => *v as usize,
                            StackValue::Boolean(v) => usize::from(*v),
                            StackValue::Null => 0,
                            _ => {
                                return Err(
                                    "PICKITEM index must be a non-negative integer".to_string()
                                );
                            }
                        };
                        match &mut item {
                            StackValue::Array(_, items) | StackValue::Struct(_, items) => {
                                let value = items
                                    .get(index)
                                    .cloned()
                                    .ok_or_else(|| "index out of range for PICKITEM".to_string())?;
                                if cfg!(target_arch = "riscv32") {
                                    core::mem::forget(core::mem::take(items));
                                }
                                stack.push(value);
                            }
                            _ => {
                                return Err(
                                    "PICKITEM expects an array, map, byte string, or integer"
                                        .to_string(),
                                );
                            }
                        }
                    }
                }
                Ok(())
            })();
            if let Err(error) = pick_result {
                if try_frames.is_empty() {
                    return Err(error);
                }
                *pending_error = Some(PendingException::message(error));
                finish!(Dispatch::Continue);
            }
        }
        SETITEM => {
            let value = pop_item(stack)?;
            let key = pop_item(stack)?;
            let mut item = pop_item(stack)?;
            ensure_nestable(&value)?;
            match &mut item {
                StackValue::ByteString(_) => {
                    return Err(
                        "SETITEM expects a mutable buffer, array, struct, or map".to_string()
                    );
                }
                StackValue::Buffer(id, bytes) => {
                    let id = *id;
                    let index = integer_value_for_collection_index(&key)?;
                    if index < 0 || (index as usize) >= bytes.len() {
                        return Err("index out of range for SETITEM".to_string());
                    }
                    let byte = match &value {
                        StackValue::Integer(value) if (-128..=255).contains(value) => *value as u8,
                        StackValue::ByteString(value) if value.len() == 1 => value[0],
                        _ => return Err("SETITEM on buffer expects a byte value".to_string()),
                    };
                    bytes[index as usize] = byte;
                    let bytes = core::mem::take(bytes);
                    let updated = StackValue::Buffer(id, bytes);
                    let affected = find_affected_indices(id, stack);
                    commit_collection_update(
                        &updated,
                        stack,
                        locals,
                        args,
                        static_fields,
                        consumed_mutations,
                        Some(&affected),
                    )?;
                }
                StackValue::Array(id, items) => {
                    let id = *id;
                    let index = integer_value_for_collection_index(&key)?;
                    if index < 0 || (index as usize) >= items.len() {
                        return Err("index out of range for SETITEM".to_string());
                    }
                    items[index as usize] = ids.clone_for_mutation(&value);
                    let items = core::mem::take(items);
                    let updated = StackValue::Array(id, items);
                    let affected = find_affected_indices(id, stack);
                    commit_collection_update(
                        &updated,
                        stack,
                        locals,
                        args,
                        static_fields,
                        consumed_mutations,
                        Some(&affected),
                    )?;
                }
                StackValue::Struct(id, items) => {
                    let id = *id;
                    let index = integer_value_for_collection_index(&key)?;
                    if index < 0 || (index as usize) >= items.len() {
                        return Err("index out of range for SETITEM".to_string());
                    }
                    items[index as usize] = ids.clone_for_mutation(&value);
                    let items = core::mem::take(items);
                    let updated = StackValue::Struct(id, items);
                    let affected = find_affected_indices(id, stack);
                    commit_collection_update(
                        &updated,
                        stack,
                        locals,
                        args,
                        static_fields,
                        consumed_mutations,
                        Some(&affected),
                    )?;
                }
                StackValue::Map(id, items) => {
                    let id = *id;
                    let mut entries = core::mem::take(items);
                    if let Some(index) = collection_rules::map_entry_index_by(
                        &entries,
                        &key,
                        primitive_key_equals,
                        validate_map_key,
                    )? {
                        entries[index].1 = ids.clone_for_mutation(&value);
                    } else {
                        entries.push((key, ids.clone_for_mutation(&value)));
                    }
                    let updated = StackValue::Map(id, entries);
                    let affected = find_affected_indices(id, stack);
                    commit_collection_update(
                        &updated,
                        stack,
                        locals,
                        args,
                        static_fields,
                        consumed_mutations,
                        Some(&affected),
                    )?;
                }
                _ => return Err("SETITEM expects an array, buffer, or map".to_string()),
            }
        }
        REMOVE => {
            let key = pop_item(stack)?;
            let mut item = pop_item(stack)?;
            match &mut item {
                StackValue::Array(id, items) => {
                    let id = *id;
                    let index = integer_value_for_collection_index(&key)?;
                    if index < 0 || (index as usize) >= items.len() {
                        return Err("index out of range for REMOVE".to_string());
                    }
                    items.remove(index as usize);
                    let items = core::mem::take(items);
                    let updated = StackValue::Array(id, items);
                    let affected = find_affected_indices(id, stack);
                    commit_collection_update(
                        &updated,
                        stack,
                        locals,
                        args,
                        static_fields,
                        consumed_mutations,
                        Some(&affected),
                    )?;
                }
                StackValue::Struct(id, items) => {
                    let id = *id;
                    let index = integer_value_for_collection_index(&key)?;
                    if index < 0 || (index as usize) >= items.len() {
                        return Err("index out of range for REMOVE".to_string());
                    }
                    items.remove(index as usize);
                    let items = core::mem::take(items);
                    let updated = StackValue::Struct(id, items);
                    let affected = find_affected_indices(id, stack);
                    commit_collection_update(
                        &updated,
                        stack,
                        locals,
                        args,
                        static_fields,
                        consumed_mutations,
                        Some(&affected),
                    )?;
                }
                StackValue::Map(id, entries) => {
                    let id = *id;
                    let mut entries = core::mem::take(entries);
                    let index = collection_rules::map_entry_index_by(
                        &entries,
                        &key,
                        primitive_key_equals,
                        validate_map_key,
                    )?
                    .ok_or_else(|| "key not found for REMOVE".to_string())?;
                    entries.remove(index);
                    let updated = StackValue::Map(id, entries);
                    let affected = find_affected_indices(id, stack);
                    commit_collection_update(
                        &updated,
                        stack,
                        locals,
                        args,
                        static_fields,
                        consumed_mutations,
                        Some(&affected),
                    )?;
                }
                _ => return Err("REMOVE expects an array, struct, or map".to_string()),
            }
        }
        CLEARITEMS => {
            let mut item = pop_item(stack)?;
            let updated = match &mut item {
                StackValue::Array(id, _) => StackValue::Array(*id, Vec::new()),
                StackValue::Struct(id, _) => StackValue::Struct(*id, Vec::new()),
                StackValue::Map(id, _) => StackValue::Map(*id, Vec::new()),
                StackValue::Buffer(id, _) => StackValue::Buffer(*id, Vec::new()),
                _ => return Err("CLEARITEMS expects a compound value".to_string()),
            };
            commit_collection_update(
                &updated,
                stack,
                locals,
                args,
                static_fields,
                consumed_mutations,
                None,
            )?;
        }
        POPITEM => {
            let mut item = pop_item(stack)?;
            match &mut item {
                StackValue::Array(id, items) => {
                    let id = *id;
                    let popped = items
                        .pop()
                        .ok_or_else(|| "POPITEM on empty array".to_string())?;
                    let items = core::mem::take(items);
                    let updated = StackValue::Array(id, items);
                    commit_collection_update(
                        &updated,
                        stack,
                        locals,
                        args,
                        static_fields,
                        consumed_mutations,
                        None,
                    )?;
                    stack.push(popped);
                }
                StackValue::Struct(id, items) => {
                    let id = *id;
                    let popped = items
                        .pop()
                        .ok_or_else(|| "POPITEM on empty struct".to_string())?;
                    let items = core::mem::take(items);
                    let updated = StackValue::Struct(id, items);
                    commit_collection_update(
                        &updated,
                        stack,
                        locals,
                        args,
                        static_fields,
                        consumed_mutations,
                        None,
                    )?;
                    stack.push(popped);
                }
                StackValue::Map(id, entries) => {
                    let id = *id;
                    let (key, value) = entries
                        .pop()
                        .ok_or_else(|| "POPITEM on empty map".to_string())?;
                    let entries = core::mem::take(entries);
                    let updated = StackValue::Map(id, entries);
                    commit_collection_update(
                        &updated,
                        stack,
                        locals,
                        args,
                        static_fields,
                        consumed_mutations,
                        None,
                    )?;
                    stack.push(key);
                    stack.push(value);
                }
                StackValue::Buffer(id, bytes) => {
                    let id = *id;
                    let byte = bytes
                        .pop()
                        .ok_or_else(|| "POPITEM on empty buffer".to_string())?;
                    let bytes = core::mem::take(bytes);
                    let updated = StackValue::Buffer(id, bytes);
                    commit_collection_update(
                        &updated,
                        stack,
                        locals,
                        args,
                        static_fields,
                        consumed_mutations,
                        None,
                    )?;
                    stack.push(StackValue::Integer(byte as i64));
                }
                _ => return Err("POPITEM expects a compound value".to_string()),
            }
        }
        CONVERT => {
            if ip + 2 > script.len() {
                return Err("truncated CONVERT operand".to_string());
            }
            let kind = script[ip + 1];
            let value = pop_item(stack)?;
            let converted = convert_value(kind, value, ids)?;
            stack.push(converted);
            ip += 2;
            finish!(Dispatch::Continue);
        }
        REVERSEITEMS => {
            let mut item = pop_item(stack)?;
            if !matches!(
                item,
                StackValue::Array(..) | StackValue::Struct(..) | StackValue::Buffer(..)
            ) {
                return Err(format!(
                    "REVERSEITEMS expects an array, struct, or buffer at ip {ip}: {item:?}"
                ));
            }
            let updated = match &mut item {
                StackValue::Array(id, items) => {
                    items.reverse();
                    let items = core::mem::take(items);
                    StackValue::Array(*id, items)
                }
                StackValue::Struct(id, items) => {
                    items.reverse();
                    let items = core::mem::take(items);
                    StackValue::Struct(*id, items)
                }
                StackValue::Buffer(id, bytes) => {
                    bytes.reverse();
                    let bytes = core::mem::take(bytes);
                    StackValue::Buffer(*id, bytes)
                }
                _ => unreachable!("REVERSEITEMS input checked above"),
            };
            commit_collection_update(
                &updated,
                stack,
                locals,
                args,
                static_fields,
                consumed_mutations,
                None,
            )?;
        }
        // =============================================================================
        // TYPE OPERATIONS (0xd9, 0xdb)
        // =============================================================================
        ISTYPE => {
            if ip + 2 > script.len() {
                return Err("truncated ISTYPE operand".to_string());
            }
            let kind = script[ip + 1];
            let item = pop_item(stack)?;
            let result = is_type(kind, &item)?;
            stack.push(StackValue::Boolean(result));
            ip += 2;
            finish!(Dispatch::Continue);
        }
        ISNULL => {
            let item = pop_item(stack)?;
            stack.push(StackValue::Boolean(is_null(&item)));
        }
        _ => unreachable!("opcode routed to compound_ops: 0x{opcode:02x}"),
    }
    *ip_ref = ip;
    Ok(Dispatch::Fallthrough)
}
