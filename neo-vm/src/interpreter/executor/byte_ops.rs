use super::super::helpers::{pop_integer, pop_item};
use super::super::opcodes::*;
use super::super::runtime_types::{propagate_update, to_abi_value, CompoundIds, StackValue};
use super::super::state::remember_consumed_mutation;
use super::control::Dispatch;
use crate::semantics::{collections as collection_rules, splice as splice_rules};
use alloc::{format, string::String, vec::Vec};

#[allow(clippy::too_many_arguments)]
#[inline]
pub(super) fn execute(
    opcode: u8,
    stack: &mut Vec<StackValue>,
    ids: &mut CompoundIds,
    locals: &mut [StackValue],
    args: &mut [StackValue],
    static_fields: &mut [StackValue],
    consumed_mutations: &mut Vec<StackValue>,
) -> Result<Dispatch, String> {
    match opcode {
        // SPLICE OPERATIONS (0x88-0x8e)
        // =============================================================================
        CAT => {
            let right_item = pop_item(stack)?;
            let left_item = pop_item(stack)?;
            let result =
                splice_rules::cat_values(&to_abi_value(&left_item), &to_abi_value(&right_item))?;
            stack.push(ids.import_abi(result));
        }
        LEFT => {
            let count = pop_integer(stack)?;
            let value = pop_item(stack)?;
            let result = splice_rules::left_value(&to_abi_value(&value), count)?;
            stack.push(ids.import_abi(result));
        }
        NEWBUFFER => {
            let count = pop_integer(stack)?;
            let buffer = collection_rules::new_buffer(count)?;
            stack.push(ids.import_abi(buffer));
        }
        RIGHT => {
            let count = pop_integer(stack)?;
            let value = pop_item(stack)?;
            let result = splice_rules::right_value(&to_abi_value(&value), count)?;
            stack.push(ids.import_abi(result));
        }
        SUBSTR => {
            let count = pop_integer(stack)?;
            let index = pop_integer(stack)?;
            let value = pop_item(stack)?;
            let result = splice_rules::substr_value(&to_abi_value(&value), index, count)?;
            stack.push(ids.import_abi(result));
        }
        MEMCPY => {
            // NeoVM MEMCPY: stack = [dst, di, src, si, count] (count on top)
            let count = pop_integer(stack)?;
            let si = pop_integer(stack)?;
            let src_item = pop_item(stack)?;
            let di = pop_integer(stack)?;
            let dst_item = pop_item(stack)?;
            let (dst_id, mut dst_bytes) = match dst_item {
                StackValue::Buffer(id, bytes) => (id, bytes),
                other => {
                    return Err(format!(
                        "MEMCPY expects buffer as destination, got {:?}",
                        other
                    ))
                }
            };
            splice_rules::memcpy_bytes(&mut dst_bytes, di, &to_abi_value(&src_item), si, count)?;
            let updated = StackValue::Buffer(dst_id, dst_bytes);
            remember_consumed_mutation(consumed_mutations, &updated);
            propagate_update(&updated, stack, locals, args, static_fields, None);
        }
        _ => unreachable!("opcode routed to byte_ops: 0x{opcode:02x}"),
    }
    Ok(Dispatch::Fallthrough)
}
