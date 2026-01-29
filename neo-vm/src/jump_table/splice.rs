//! Splice operations for the Neo Virtual Machine.
//!
//! This module provides the splice operation handlers for the Neo VM.

use crate::error::VmError;
use crate::error::VmResult;
use crate::execution_engine::ExecutionEngine;
use crate::instruction::Instruction;
use crate::jump_table::JumpTable;
use crate::op_code::OpCode;
use crate::stack_item::StackItem;
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
        .as_int()?
        .to_usize()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid buffer size"))?;

    // Create a new buffer
    let buffer = StackItem::from_buffer(vec![0; size]);

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
    let count = context
        .pop()?
        .as_int()?
        .to_usize()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid count"))?;
    let src_offset = context
        .pop()?
        .as_int()?
        .to_usize()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid source offset"))?;
    let dst_offset = context
        .pop()?
        .as_int()?
        .to_usize()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid destination offset"))?;
    let src = context.pop()?;
    let dst = context.pop()?;

    // Resolve the source data without taking ownership when possible
    let src_bytes = match &src {
        StackItem::ByteString(data) => std::borrow::Cow::Borrowed(data.as_slice()),
        StackItem::Buffer(buffer) => std::borrow::Cow::Owned(buffer.data()),
        _ => {
            return Err(VmError::invalid_type_simple(
                "Expected ByteString or Buffer for source",
            ));
        }
    };
    let src_view = src_bytes.as_ref();

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
    // SECURITY FIX (M-2): Get max_item_size limit before borrowing context mutably
    let max_item_size = engine.limits().max_item_size as usize;

    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the values from the stack
    let b = context.pop()?;
    let a = context.pop()?;

    // Concatenate the values and enforce MaxItemSize limit
    let result = match (a, b) {
        (StackItem::ByteString(mut a), StackItem::ByteString(b)) => {
            a.extend_from_slice(&b);
            // SECURITY FIX (M-2): Enforce MaxItemSize after concatenation
            if a.len() > max_item_size {
                return Err(VmError::invalid_operation_msg(format!(
                    "MaxItemSize exceed: {}/{}",
                    a.len(),
                    max_item_size
                )));
            }
            StackItem::from_byte_string(a)
        }
        (StackItem::Buffer(a), StackItem::Buffer(b)) => {
            a.extend_from_slice(&b.data());
            // SECURITY FIX (M-2): Enforce MaxItemSize after concatenation
            if a.len() > max_item_size {
                return Err(VmError::invalid_operation_msg(format!(
                    "MaxItemSize exceed: {}/{}",
                    a.len(),
                    max_item_size
                )));
            }
            StackItem::Buffer(a)
        }
        (StackItem::ByteString(mut a), StackItem::Buffer(b)) => {
            a.extend_from_slice(&b.data());
            // SECURITY FIX (M-2): Enforce MaxItemSize after concatenation
            if a.len() > max_item_size {
                return Err(VmError::invalid_operation_msg(format!(
                    "MaxItemSize exceed: {}/{}",
                    a.len(),
                    max_item_size
                )));
            }
            StackItem::from_byte_string(a)
        }
        (StackItem::Buffer(a), StackItem::ByteString(b)) => {
            a.extend_from_slice(&b);
            // SECURITY FIX (M-2): Enforce MaxItemSize after concatenation
            if a.len() > max_item_size {
                return Err(VmError::invalid_operation_msg(format!(
                    "MaxItemSize exceed: {}/{}",
                    a.len(),
                    max_item_size
                )));
            }
            StackItem::Buffer(a)
        }
        _ => {
            return Err(VmError::invalid_type_simple(
                "Expected ByteString or Buffer",
            ));
        }
    };

    context.push(result)?;

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
        .as_int()?
        .to_usize()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid count"))?;
    let offset = context
        .pop()?
        .as_int()?
        .to_usize()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid offset"))?;
    let value = context.pop()?;
    let data = value.as_bytes()?;
    if offset + count > data.len() {
        return Err(VmError::invalid_operation_msg(format!(
            "Substring out of bounds: {} + {} > {}",
            offset,
            count,
            data.len()
        )));
    }

    let substring = data[offset..offset + count].to_vec();
    context.push(StackItem::from_buffer(substring))?;

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
        .as_int()?
        .to_usize()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid count"))?;
    let value = context.pop()?;
    let data = value.as_bytes()?;
    if count > data.len() {
        return Err(VmError::invalid_operation_msg(format!(
            "Left out of bounds: {} > {}",
            count,
            data.len()
        )));
    }

    let left = data[..count].to_vec();
    context.push(StackItem::from_buffer(left))?;

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
        .as_int()?
        .to_usize()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid count"))?;
    let value = context.pop()?;
    let data = value.as_bytes()?;
    if count > data.len() {
        return Err(VmError::invalid_operation_msg(format!(
            "Right out of bounds: {} > {}",
            count,
            data.len()
        )));
    }

    let right = data[data.len() - count..].to_vec();
    context.push(StackItem::from_buffer(right))?;

    Ok(())
}
