//! Splice operations for the Neo Virtual Machine.
//!
//! This module provides the splice operation handlers for the Neo VM.

use crate::error::VmError;
use crate::error::VmResult;
use crate::execution_engine::ExecutionEngine;
use crate::jump_table::{JumpTable, register_jump_handlers, require_context};
use crate::stack_item::StackItem;
use crate::{Instruction, OpCode};
use num_traits::ToPrimitive;

fn i32_operand(item: StackItem, name: &str) -> VmResult<i32> {
    super::get_integer(item)?
        .to_i32()
        .ok_or_else(|| VmError::invalid_operation_msg(format!("Invalid {name}")))
}

fn non_negative(value: i32, name: &str) -> VmResult<usize> {
    usize::try_from(value)
        .map_err(|_| VmError::invalid_operation_msg(format!("The {name} can not be negative")))
}

fn checked_end(start: usize, count: usize, len: usize, name: &str) -> VmResult<usize> {
    let end = start
        .checked_add(count)
        .ok_or_else(|| VmError::invalid_operation_msg(format!("{name} range out of bounds")))?;
    if end > len {
        return Err(VmError::invalid_operation_msg(format!(
            "{name} range out of bounds"
        )));
    }
    Ok(end)
}

/// Local equivalent of C# `StackItem.GetSpan()` for byte-oriented opcodes.
fn span_bytes(item: StackItem) -> VmResult<Vec<u8>> {
    match item {
        StackItem::Boolean(value) => Ok(vec![u8::from(value)]),
        StackItem::Integer(value) if value.is_zero() => Ok(Vec::new()),
        StackItem::Integer(value) => Ok(value.to_signed_bytes_le()),
        StackItem::ByteString(bytes) => Ok(bytes),
        StackItem::Buffer(buffer) => Ok(buffer.data()),
        _ => Err(VmError::invalid_type_simple(
            "Stack item does not expose byte memory",
        )),
    }
}

/// Registers the splice operation handlers.
pub fn register_handlers<S>(jump_table: &mut JumpTable<S>) {
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
fn new_buffer<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    let max_item_size = engine.limits().max_item_size;
    let context = require_context(engine)?;

    let size = i32_operand(context.pop()?, "buffer size")?;
    if size < 0 || size as u32 > max_item_size {
        return Err(VmError::invalid_operation_msg(format!(
            "MaxItemSize exceed: {size}/{max_item_size}"
        )));
    }
    let buffer = StackItem::from_buffer(vec![0; size as usize]);

    // Push the buffer onto the stack
    context.push(buffer)?;

    Ok(())
}

/// Implements the MEMCPY operation.
fn memcpy<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    let context = require_context(engine)?;

    // Pop order matches C#: count, src_index, src, dst_index, dst
    let count = non_negative(i32_operand(context.pop()?, "count")?, "count")?;
    let src_offset = non_negative(
        i32_operand(context.pop()?, "source offset")?,
        "source index",
    )?;
    let src = span_bytes(context.pop()?)?;
    let src_end = checked_end(src_offset, count, src.len(), "Source")?;

    let dst_offset = non_negative(
        i32_operand(context.pop()?, "destination offset")?,
        "destination index",
    )?;
    let dst = match context.pop()? {
        StackItem::Buffer(buffer) => buffer,
        _ => {
            return Err(VmError::invalid_type_simple(
                "Expected Buffer for destination",
            ));
        }
    };
    let dst_end = checked_end(dst_offset, count, dst.len(), "Destination")?;

    dst.with_data_mut(|bytes| {
        bytes[dst_offset..dst_end].copy_from_slice(&src[src_offset..src_end]);
    });

    Ok(())
}

/// Implements the CAT operation.
///
/// # Security Note
/// This operation enforces `MaxItemSize` limits after concatenation to prevent
/// memory exhaustion attacks via incremental `ByteString` building.
fn cat<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    let max_item_size = engine.limits().max_item_size as usize;
    let context = require_context(engine)?;

    let x2 = span_bytes(context.pop()?)?;
    let x1 = span_bytes(context.pop()?)?;

    let result_len = x1
        .len()
        .checked_add(x2.len())
        .ok_or_else(|| VmError::invalid_operation_msg("CAT result size overflow"))?;
    if result_len > max_item_size {
        return Err(VmError::invalid_operation_msg(format!(
            "MaxItemSize exceed: {}/{}",
            result_len, max_item_size
        )));
    }
    let mut result = Vec::with_capacity(result_len);
    result.extend_from_slice(&x1);
    result.extend_from_slice(&x2);
    context.push(StackItem::from_buffer(result))?;

    Ok(())
}

/// Implements the SUBSTR operation.
fn substr<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    let context = require_context(engine)?;

    let count = non_negative(i32_operand(context.pop()?, "count")?, "count")?;
    let offset = non_negative(i32_operand(context.pop()?, "offset")?, "index")?;
    let value = span_bytes(context.pop()?)?;
    let end = checked_end(offset, count, value.len(), "SUBSTR")?;
    context.push(StackItem::from_buffer(value[offset..end].to_vec()))?;

    Ok(())
}

/// Pre-`HF_Echidna` SUBSTR implementation from C#
/// `ApplicationEngine.VulnerableSubStr`. Its `index + count` comparison uses
/// wrapping `i32` arithmetic; the subsequent safe range check preserves the
/// reference implementation's fault without exposing unchecked memory access.
pub(crate) fn vulnerable_substr<S>(
    engine: &mut ExecutionEngine<S>,
    _instruction: &Instruction,
) -> VmResult<()> {
    let context = require_context(engine)?;

    let count = i32_operand(context.pop()?, "count")?;
    if count < 0 {
        return Err(VmError::invalid_operation_msg(
            "The count can not be negative for SUBSTR",
        ));
    }
    let offset = i32_operand(context.pop()?, "offset")?;
    if offset < 0 {
        return Err(VmError::invalid_operation_msg(
            "The index can not be negative for SUBSTR",
        ));
    }
    let value = span_bytes(context.pop()?)?;
    let value_len = i32::try_from(value.len())
        .map_err(|_| VmError::invalid_operation_msg("SUBSTR source length exceeds Int32"))?;
    if offset.wrapping_add(count) > value_len {
        return Err(VmError::invalid_operation_msg("SUBSTR range out of bounds"));
    }

    let offset = offset as usize;
    let count = count as usize;
    let end = checked_end(offset, count, value.len(), "SUBSTR")?;
    context.push(StackItem::from_buffer(value[offset..end].to_vec()))?;

    Ok(())
}

/// Implements the LEFT operation.
fn left<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    let context = require_context(engine)?;

    let count = non_negative(i32_operand(context.pop()?, "count")?, "count")?;
    let value = span_bytes(context.pop()?)?;
    let end = checked_end(0, count, value.len(), "LEFT")?;
    context.push(StackItem::from_buffer(value[..end].to_vec()))?;

    Ok(())
}

/// Implements the RIGHT operation.
fn right<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    let context = require_context(engine)?;

    let count = non_negative(i32_operand(context.pop()?, "count")?, "count")?;
    let value = span_bytes(context.pop()?)?;
    if count > value.len() {
        return Err(VmError::invalid_operation_msg(format!(
            "RIGHT count out of range: {count}/[0, {})",
            value.len()
        )));
    }
    context.push(StackItem::from_buffer(
        value[value.len() - count..].to_vec(),
    ))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::Script;
    use crate::stack_item::Buffer;

    fn engine_with_stack(items: Vec<StackItem>) -> ExecutionEngine {
        let mut engine = ExecutionEngine::<()>::new(None);
        engine
            .load_script(Script::new_relaxed(vec![OpCode::RET.byte()]), -1, 0)
            .expect("load test script");
        let context = engine.current_context_mut().expect("current context");
        for item in items {
            context.push(item).expect("push test item");
        }
        engine
    }

    fn instruction(opcode: OpCode) -> Instruction {
        Instruction::new(opcode, &[])
    }

    fn pop(engine: &mut ExecutionEngine) -> StackItem {
        engine
            .current_context_mut()
            .expect("current context")
            .pop()
            .expect("result item")
    }

    #[test]
    fn local_span_matches_neovm_primitive_memory() {
        assert_eq!(
            span_bytes(StackItem::from_i64(0)).unwrap(),
            Vec::<u8>::new()
        );
        assert_eq!(span_bytes(StackItem::from_bool(false)).unwrap(), vec![0]);
        assert_eq!(span_bytes(StackItem::from_bool(true)).unwrap(), vec![1]);
        assert_eq!(
            span_bytes(StackItem::from_i64(-129)).unwrap(),
            vec![0x7f, 0xff]
        );
    }

    #[test]
    fn cat_preserves_operand_order_and_returns_buffer() {
        let mut engine = engine_with_stack(vec![
            StackItem::from_byte_string(vec![1, 2]),
            StackItem::from_bool(false),
        ]);

        cat(&mut engine, &instruction(OpCode::CAT)).expect("CAT succeeds");

        match pop(&mut engine) {
            StackItem::Buffer(buffer) => assert_eq!(buffer.data(), vec![1, 2, 0]),
            other => panic!("expected Buffer, got {other:?}"),
        }
    }

    #[test]
    fn memcpy_handles_overlapping_aliases_like_span_copy_to() {
        let buffer = Buffer::new(vec![1, 2, 3, 4]);
        let mut engine = engine_with_stack(vec![
            StackItem::Buffer(buffer.clone()),
            StackItem::from_i64(1),
            StackItem::Buffer(buffer.clone()),
            StackItem::from_i64(0),
            StackItem::from_i64(3),
        ]);

        memcpy(&mut engine, &instruction(OpCode::MEMCPY)).expect("MEMCPY succeeds");

        assert_eq!(buffer.data(), vec![1, 1, 2, 3]);
    }

    #[test]
    fn vulnerable_substr_preserves_valid_results_and_faults_safely_on_wrapped_end() {
        let mut engine = engine_with_stack(vec![
            StackItem::from_byte_string(vec![1, 2, 3]),
            StackItem::from_i64(1),
            StackItem::from_i64(2),
        ]);
        vulnerable_substr(&mut engine, &instruction(OpCode::SUBSTR))
            .expect("valid pre-Echidna SUBSTR succeeds");
        match pop(&mut engine) {
            StackItem::Buffer(buffer) => assert_eq!(buffer.data(), vec![2, 3]),
            other => panic!("expected Buffer, got {other:?}"),
        }

        let mut engine = engine_with_stack(vec![
            StackItem::from_byte_string(vec![1]),
            StackItem::from_i64(i64::from(i32::MAX)),
            StackItem::from_i64(1),
        ]);
        assert!(
            vulnerable_substr(&mut engine, &instruction(OpCode::SUBSTR)).is_err(),
            "wrapped i32 end must still fault without unchecked memory access"
        );
    }
}
