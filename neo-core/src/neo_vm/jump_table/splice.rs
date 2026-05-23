//! Splice operations for the Neo Virtual Machine.
//!
//! This module provides the splice operation handlers for the Neo VM.

use crate::neo_vm::error::VmError;
use crate::neo_vm::error::VmResult;
use crate::neo_vm::execution_engine::ExecutionEngine;
use crate::neo_vm::instruction::Instruction;
use crate::neo_vm::jump_table::JumpTable;
use crate::neo_vm::stack_item::StackItem;
use neo_vm_rs::OpCode;
use num_traits::ToPrimitive;

/// Registers the splice operation handlers.
pub fn register_handlers(jump_table: &mut JumpTable) {
    jump_table.register(OpCode::NEWBUFFER, new_buffer);
    jump_table.register(OpCode::MEMCPY, memcpy);
    jump_table.register(OpCode::CAT, cat);
    jump_table.register(OpCode::SUBSTR, substr);
    jump_table.register(OpCode::LEFT, left);
    jump_table.register(OpCode::RIGHT, right);
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
        .to_usize()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid count"))?;
    let src_offset = context
        .pop()?
        .into_int()?
        .to_usize()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid source offset"))?;
    let src = context.pop()?;
    let dst_offset = context
        .pop()?
        .into_int()?
        .to_usize()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid destination offset"))?;
    let dst = context.pop()?;

    let src_value = neo_vm_rs::StackValue::try_from(src)
        .map_err(|_| VmError::invalid_type_simple("Expected ByteString or Buffer for source"))?;
    let src_view = neo_vm_rs::byte_sequence_bytes(&src_value)
        .ok_or_else(|| VmError::invalid_type_simple("Expected ByteString or Buffer for source"))?;

    // Check bounds
    if src_offset + count > src_view.len() {
        return Err(VmError::invalid_operation_msg(format!(
            "Source out of bounds: {} + {} > {}",
            src_offset,
            count,
            src_view.len()
        )));
    }

    // Get the destination data
    match dst {
        StackItem::Buffer(buffer) => {
            // Check bounds
            if dst_offset + count > buffer.len() {
                return Err(VmError::invalid_operation_msg(format!(
                    "Destination out of bounds: {} + {} > {}",
                    dst_offset,
                    count,
                    buffer.len()
                )));
            }

            // Copy the data
            buffer.with_data_mut(|dst_data| {
                dst_data[dst_offset..dst_offset + count]
                    .copy_from_slice(&src_view[src_offset..src_offset + count]);
            });
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

    // Match C# semantics: CAT always creates a brand-new buffer and never mutates
    // either operand in place.
    let x2 = context.pop()?.into_bytes()?;
    let x1 = context.pop()?.into_bytes()?;

    let length = x1.len().saturating_add(x2.len());
    if length > max_item_size {
        return Err(VmError::invalid_operation_msg(format!(
            "MaxItemSize exceed: {}/{}",
            length, max_item_size
        )));
    }

    let result = neo_vm_rs::concat_byte_sequences(
        neo_vm_rs::StackValue::Buffer(x1),
        neo_vm_rs::StackValue::Buffer(x2),
    )
    .ok_or_else(|| VmError::invalid_type_simple("Expected ByteString or Buffer"))?;
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
        .to_usize()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid count"))?;
    let offset = context
        .pop()?
        .into_int()?
        .to_usize()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid offset"))?;
    let value = context.pop()?;
    let data = value.into_bytes()?;
    if offset + count > data.len() {
        return Err(VmError::invalid_operation_msg(format!(
            "Substring out of bounds: {} + {} > {}",
            offset,
            count,
            data.len()
        )));
    }

    let substring =
        neo_vm_rs::slice_byte_sequence(neo_vm_rs::StackValue::Buffer(data), offset, count)
            .ok_or_else(|| VmError::invalid_operation_msg("Substring out of bounds"))?;
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
        .to_usize()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid count"))?;
    let value = context.pop()?;
    let data = value.into_bytes()?;
    if count > data.len() {
        return Err(VmError::invalid_operation_msg(format!(
            "Left out of bounds: {} > {}",
            count,
            data.len()
        )));
    }

    let left = neo_vm_rs::slice_byte_sequence(neo_vm_rs::StackValue::Buffer(data), 0, count)
        .ok_or_else(|| VmError::invalid_operation_msg("Left out of bounds"))?;
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
        .to_usize()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid count"))?;
    let value = context.pop()?;
    let data = value.into_bytes()?;
    if count > data.len() {
        return Err(VmError::invalid_operation_msg(format!(
            "Right out of bounds: {} > {}",
            count,
            data.len()
        )));
    }

    let start = data.len() - count;
    let right = neo_vm_rs::slice_byte_sequence(neo_vm_rs::StackValue::Buffer(data), start, count)
        .ok_or_else(|| VmError::invalid_operation_msg("Right out of bounds"))?;
    context.push(StackItem::try_from(right)?)?;

    Ok(())
}
