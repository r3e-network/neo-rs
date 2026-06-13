//! Splice operations for the Neo Virtual Machine.
//!
//! This module provides the splice operation handlers for the Neo VM.

use crate::error::VmError;
use crate::error::VmResult;
use crate::execution_engine::ExecutionEngine;
use crate::jump_table::{JumpTable, register_jump_handlers};
use crate::stack_item::StackItem;
use neo_vm_rs::{Instruction, OpCode, semantics::splice as splice_rules};
use num_traits::ToPrimitive;

/// Registers the splice operation handlers.
pub fn register_handlers(jump_table: &mut JumpTable) {
    register_jump_handlers![
        jump_table;
        OpCode::NEWBUFFER => new_buffer,
        OpCode::MEMCPY => memcpy,
        OpCode::CAT => cat,
        OpCode::SUBSTR => substr,
        OpCode::LEFT => left,
        OpCode::RIGHT => right,
    ];
}

/// Implements the NEWBUFFER operation.
fn new_buffer(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the size from the stack
    let size = context
        .pop()?
        .into_int()?
        .to_i64()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid buffer size"))?;

    let buffer = StackItem::try_from(
        neo_vm_rs::semantics::collections::new_buffer(size)
            .map_err(VmError::invalid_operation_msg)?,
    )?;

    // Push the buffer onto the stack
    context.push(buffer)?;

    Ok(())
}

/// Implements the MEMCPY operation.
fn memcpy(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the values from the stack
    // Pop order matches C#: count, src_index, src, dst_index, dst
    let count = context
        .pop()?
        .into_int()?
        .to_i64()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid count"))?;
    let src_offset = context
        .pop()?
        .into_int()?
        .to_i64()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid source offset"))?;
    let src = context.pop()?;
    let dst_offset = context
        .pop()?
        .into_int()?
        .to_i64()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid destination offset"))?;
    let dst = context.pop()?;

    let src_value = neo_vm_rs::StackValue::try_from(src)
        .map_err(|_| VmError::invalid_type_simple("Expected ByteString or Buffer for source"))?;

    // Get the destination data
    match dst {
        StackItem::Buffer(buffer) => {
            buffer
                .with_data_mut(|dst_data| {
                    splice_rules::memcpy_bytes(dst_data, dst_offset, &src_value, src_offset, count)
                })
                .map_err(VmError::invalid_operation_msg)?;
        }
        _ => {
            return Err(VmError::invalid_type_simple(
                "Expected Buffer for destination",
            ));
        }
    }

    Ok(())
}

/// Implements the CAT operation.
///
/// # Security Note
/// This operation enforces `MaxItemSize` limits after concatenation to prevent
/// memory exhaustion attacks via incremental `ByteString` building.
fn cat(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let max_item_size = engine.limits().max_item_size as usize;
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    let x2 = neo_vm_rs::StackValue::try_from(context.pop()?)
        .map_err(|_| VmError::invalid_type_simple("Expected GetSpan-compatible CAT operand"))?;
    let x1 = neo_vm_rs::StackValue::try_from(context.pop()?)
        .map_err(|_| VmError::invalid_type_simple("Expected GetSpan-compatible CAT operand"))?;

    let result = splice_rules::cat_values(&x1, &x2).map_err(VmError::invalid_operation_msg)?;
    let result_len = match &result {
        neo_vm_rs::StackValue::Buffer(bytes) => bytes.len(),
        _ => 0,
    };
    if result_len > max_item_size {
        return Err(VmError::invalid_operation_msg(format!(
            "MaxItemSize exceed: {}/{}",
            result_len, max_item_size
        )));
    }
    context.push(StackItem::try_from(result)?)?;

    Ok(())
}

/// Implements the SUBSTR operation.
fn substr(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the values from the stack
    let count = context
        .pop()?
        .into_int()?
        .to_i64()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid count"))?;
    let offset = context
        .pop()?
        .into_int()?
        .to_i64()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid offset"))?;
    let value = context.pop()?;
    let value = neo_vm_rs::StackValue::try_from(value)
        .map_err(|_| VmError::invalid_type_simple("Expected GetSpan-compatible SUBSTR value"))?;
    let substring = splice_rules::substr_value(&value, offset, count)
        .map_err(VmError::invalid_operation_msg)?;
    context.push(StackItem::try_from(substring)?)?;

    Ok(())
}

/// Implements the LEFT operation.
fn left(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the values from the stack
    let count = context
        .pop()?
        .into_int()?
        .to_i64()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid count"))?;
    let value = context.pop()?;
    let value = neo_vm_rs::StackValue::try_from(value)
        .map_err(|_| VmError::invalid_type_simple("Expected GetSpan-compatible LEFT value"))?;
    let left = splice_rules::left_value(&value, count).map_err(VmError::invalid_operation_msg)?;
    context.push(StackItem::try_from(left)?)?;

    Ok(())
}

/// Implements the RIGHT operation.
fn right(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the values from the stack
    let count = context
        .pop()?
        .into_int()?
        .to_i64()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid count"))?;
    let value = context.pop()?;
    let value = neo_vm_rs::StackValue::try_from(value)
        .map_err(|_| VmError::invalid_type_simple("Expected GetSpan-compatible RIGHT value"))?;
    let right = splice_rules::right_value(&value, count).map_err(VmError::invalid_operation_msg)?;
    context.push(StackItem::try_from(right)?)?;

    Ok(())
}
