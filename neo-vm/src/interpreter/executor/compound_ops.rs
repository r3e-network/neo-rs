use super::super::helpers::*;
use super::super::opcodes::*;
use super::super::runtime_types::{
    find_affected_indices, propagate_update, CompoundIds, StackValue,
};
use super::super::state::{remember_consumed_mutation, PendingException, TryStack, MAX_STACK_SIZE};
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
) {
    remember_consumed_mutation(consumed_mutations, updated);
    propagate_update(
        updated,
        stack,
        locals,
        args,
        static_fields,
        affected_stack_indices,
    );
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

            let mut items = Vec::with_capacity(count);
            for _ in 0..count {
                items.push(pop_item(stack)?);
            }
            let items = items
                .into_iter()
                .map(|item| ids.clone_struct_for_storage(&item))
                .collect();
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

            let mut items = Vec::with_capacity(count);
            for _ in 0..count {
                items.push(pop_item(stack)?);
            }
            let items = items
                .into_iter()
                .map(|item| ids.clone_struct_for_storage(&item))
                .collect();
            stack.push(ids.array(items));
        }
        UNPACK => {
            let item = pop_item(stack)?;
            match item {
                StackValue::Array(_, items) | StackValue::Struct(_, items) => {
                    let count = items.len() as i64;
                    for item in items.into_iter().rev() {
                        stack.push(item);
                    }
                    stack.push(StackValue::Integer(count));
                }
                StackValue::Map(_, items) => {
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
            let has_key = collection_rules::has_key(&to_abi_value(&item), &to_abi_value(&key))
                .map_err(|_| "HASKEY expects an array, buffer, or map".to_string())?;
            stack.push(StackValue::Boolean(has_key));
        }
        KEYS => {
            let item = pop_item(stack)?;
            match item {
                StackValue::Map(_, items) => {
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
            let item = pop_item(stack)?;
            match item {
                StackValue::Map(_, items) => {
                    let values = items
                        .into_iter()
                        .map(|(_, value)| value)
                        .collect::<Vec<_>>();
                    stack.push(ids.array(values));
                }
                StackValue::Array(_, items) => {
                    stack.push(ids.array(items));
                }
                StackValue::Struct(_, items) => {
                    stack.push(ids.array(items));
                }
                _ => return Err("VALUES expects a map, array, or struct".to_string()),
            }
        }
        APPEND => {
            let value = pop_item(stack)?;
            let item = pop_item(stack)?;
            match item {
                StackValue::Array(id, mut items) => {
                    items.push(ids.clone_struct_for_storage(&value));
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
                    );
                }
                StackValue::Struct(id, mut items) => {
                    items.push(ids.clone_struct_for_storage(&value));
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
                    );
                }
                _ => return Err("APPEND expects an array or struct".to_string()),
            }
        }
        PICKITEM => {
            let pick_result = (|| -> Result<(), String> {
                let key_or_index = pop_item(stack)?;
                let item = pop_item(stack)?;
                match item {
                    StackValue::Map(_, items) => {
                        // Map key can be any primitive type
                        let index = collection_rules::map_entry_index_by(
                            &items,
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
                        let index = match key_or_index {
                            StackValue::Integer(v) if v >= 0 => v as usize,
                            StackValue::Boolean(v) => {
                                if v {
                                    1
                                } else {
                                    0
                                }
                            }
                            StackValue::Null => 0,
                            _ => {
                                return Err(
                                    "PICKITEM index must be a non-negative integer".to_string()
                                )
                            }
                        };
                        match item {
                            StackValue::Array(_, items) | StackValue::Struct(_, items) => {
                                let value = items
                                    .get(index)
                                    .cloned()
                                    .ok_or_else(|| "index out of range for PICKITEM".to_string())?;
                                if cfg!(target_arch = "riscv32") {
                                    core::mem::forget(items);
                                }
                                stack.push(value);
                            }
                            _ => {
                                return Err(
                                    "PICKITEM expects an array, map, byte string, or integer"
                                        .to_string(),
                                )
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
            let item = pop_item(stack)?;
            match item {
                StackValue::ByteString(_) => {
                    return Err(
                        "SETITEM expects a mutable buffer, array, struct, or map".to_string()
                    )
                }
                StackValue::Buffer(id, mut bytes) => {
                    let index = integer_value_for_collection_index(&key)?;
                    if index < 0 || (index as usize) >= bytes.len() {
                        return Err("index out of range for SETITEM".to_string());
                    }
                    let byte = match value {
                        StackValue::Integer(value) if (-128..=255).contains(&value) => value as u8,
                        StackValue::ByteString(value) if value.len() == 1 => value[0],
                        _ => return Err("SETITEM on buffer expects a byte value".to_string()),
                    };
                    bytes[index as usize] = byte;
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
                    );
                }
                StackValue::Array(id, mut items) => {
                    let index = integer_value_for_collection_index(&key)?;
                    if index < 0 || (index as usize) >= items.len() {
                        return Err("index out of range for SETITEM".to_string());
                    }
                    items[index as usize] = ids.clone_struct_for_storage(&value);
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
                    );
                }
                StackValue::Struct(id, mut items) => {
                    let index = integer_value_for_collection_index(&key)?;
                    if index < 0 || (index as usize) >= items.len() {
                        return Err("index out of range for SETITEM".to_string());
                    }
                    items[index as usize] = ids.clone_struct_for_storage(&value);
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
                    );
                }
                StackValue::Map(id, mut items) => {
                    if let Some(index) = collection_rules::map_entry_index_by(
                        &items,
                        &key,
                        primitive_key_equals,
                        validate_map_key,
                    )? {
                        items[index].1 = ids.clone_struct_for_storage(&value);
                    } else {
                        let mut updated_items = Vec::with_capacity(items.len() + 1);
                        updated_items.extend(items);
                        updated_items.push((key, ids.clone_struct_for_storage(&value)));
                        items = updated_items;
                    }
                    let updated = StackValue::Map(id, items);
                    let affected = find_affected_indices(id, stack);
                    commit_collection_update(
                        &updated,
                        stack,
                        locals,
                        args,
                        static_fields,
                        consumed_mutations,
                        Some(&affected),
                    );
                }
                _ => return Err("SETITEM expects an array, buffer, or map".to_string()),
            }
        }
        REMOVE => {
            let key = pop_item(stack)?;
            let item = pop_item(stack)?;
            match item {
                StackValue::Array(id, mut items) => {
                    let index = integer_value_for_collection_index(&key)?;
                    if index < 0 || (index as usize) >= items.len() {
                        return Err("index out of range for REMOVE".to_string());
                    }
                    items.remove(index as usize);
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
                    );
                }
                StackValue::Struct(id, mut items) => {
                    let index = integer_value_for_collection_index(&key)?;
                    if index < 0 || (index as usize) >= items.len() {
                        return Err("index out of range for REMOVE".to_string());
                    }
                    items.remove(index as usize);
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
                    );
                }
                StackValue::Map(id, mut items) => {
                    let index = collection_rules::map_entry_index_by(
                        &items,
                        &key,
                        primitive_key_equals,
                        validate_map_key,
                    )?
                    .ok_or_else(|| "key not found for REMOVE".to_string())?;
                    items.remove(index);
                    let updated = StackValue::Map(id, items);
                    let affected = find_affected_indices(id, stack);
                    commit_collection_update(
                        &updated,
                        stack,
                        locals,
                        args,
                        static_fields,
                        consumed_mutations,
                        Some(&affected),
                    );
                }
                _ => return Err("REMOVE expects an array, struct, or map".to_string()),
            }
        }
        CLEARITEMS => {
            let item = pop_item(stack)?;
            match item {
                StackValue::Array(id, _) => {
                    let updated = StackValue::Array(id, Vec::new());
                    commit_collection_update(
                        &updated,
                        stack,
                        locals,
                        args,
                        static_fields,
                        consumed_mutations,
                        None,
                    );
                }
                StackValue::Struct(id, _) => {
                    let updated = StackValue::Struct(id, Vec::new());
                    commit_collection_update(
                        &updated,
                        stack,
                        locals,
                        args,
                        static_fields,
                        consumed_mutations,
                        None,
                    );
                }
                StackValue::Map(id, _) => {
                    let updated = StackValue::Map(id, Vec::new());
                    commit_collection_update(
                        &updated,
                        stack,
                        locals,
                        args,
                        static_fields,
                        consumed_mutations,
                        None,
                    );
                }
                StackValue::Buffer(id, _) => {
                    let updated = StackValue::Buffer(id, Vec::new());
                    commit_collection_update(
                        &updated,
                        stack,
                        locals,
                        args,
                        static_fields,
                        consumed_mutations,
                        None,
                    );
                }
                _ => return Err("CLEARITEMS expects a compound value".to_string()),
            }
        }
        POPITEM => {
            let item = pop_item(stack)?;
            match item {
                StackValue::Array(id, mut items) => {
                    let popped = items
                        .pop()
                        .ok_or_else(|| "POPITEM on empty array".to_string())?;
                    let updated = StackValue::Array(id, items);
                    commit_collection_update(
                        &updated,
                        stack,
                        locals,
                        args,
                        static_fields,
                        consumed_mutations,
                        None,
                    );
                    stack.push(popped);
                }
                StackValue::Struct(id, mut items) => {
                    let popped = items
                        .pop()
                        .ok_or_else(|| "POPITEM on empty struct".to_string())?;
                    let updated = StackValue::Struct(id, items);
                    commit_collection_update(
                        &updated,
                        stack,
                        locals,
                        args,
                        static_fields,
                        consumed_mutations,
                        None,
                    );
                    stack.push(popped);
                }
                StackValue::Map(id, mut entries) => {
                    let (key, value) = entries
                        .pop()
                        .ok_or_else(|| "POPITEM on empty map".to_string())?;
                    let updated = StackValue::Map(id, entries);
                    commit_collection_update(
                        &updated,
                        stack,
                        locals,
                        args,
                        static_fields,
                        consumed_mutations,
                        None,
                    );
                    stack.push(key);
                    stack.push(value);
                }
                StackValue::Buffer(id, mut bytes) => {
                    let byte = bytes
                        .pop()
                        .ok_or_else(|| "POPITEM on empty buffer".to_string())?;
                    let updated = StackValue::Buffer(id, bytes);
                    commit_collection_update(
                        &updated,
                        stack,
                        locals,
                        args,
                        static_fields,
                        consumed_mutations,
                        None,
                    );
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
            let item = pop_item(stack)?;
            match item {
                StackValue::Array(id, mut items) => {
                    items.reverse();
                    let updated = StackValue::Array(id, items);
                    commit_collection_update(
                        &updated,
                        stack,
                        locals,
                        args,
                        static_fields,
                        consumed_mutations,
                        None,
                    );
                }
                StackValue::Struct(id, mut items) => {
                    items.reverse();
                    let updated = StackValue::Struct(id, items);
                    commit_collection_update(
                        &updated,
                        stack,
                        locals,
                        args,
                        static_fields,
                        consumed_mutations,
                        None,
                    );
                }
                StackValue::Buffer(id, mut bytes) => {
                    bytes.reverse();
                    let updated = StackValue::Buffer(id, bytes);
                    commit_collection_update(
                        &updated,
                        stack,
                        locals,
                        args,
                        static_fields,
                        consumed_mutations,
                        None,
                    );
                }
                _ => {
                    return Err(format!(
                        "REVERSEITEMS expects an array, struct, or buffer at ip {ip}: {item:?}"
                    ));
                }
            }
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
