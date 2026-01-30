//! Push operations for the Neo Virtual Machine.
//!
//! This module provides the push operation handlers for the Neo VM.

use crate::error::VmError;
use crate::error::VmResult;
use crate::execution_engine::ExecutionEngine;
use crate::instruction::Instruction;
use crate::jump_table::JumpTable;
use crate::op_code::OpCode;
use crate::stack_item::StackItem;
use num_bigint::BigInt;

const HASH_SIZE: usize = 32;

/// Registers the push operation handlers.
pub fn register_handlers(jump_table: &mut JumpTable) {
    jump_table.register(OpCode::PUSHINT8, push_int8);
    jump_table.register(OpCode::PUSHINT16, push_int16);
    jump_table.register(OpCode::PUSHINT32, push_int32);
    jump_table.register(OpCode::PUSHINT64, push_int64);
    jump_table.register(OpCode::PUSHINT128, push_int128);
    jump_table.register(OpCode::PUSHINT256, push_int256);
    jump_table.register(OpCode::PUSHA, push_a);
    jump_table.register(OpCode::PUSHNULL, push_null);
    jump_table.register(OpCode::PUSHDATA1, push_data);
    jump_table.register(OpCode::PUSHDATA2, push_data);
    jump_table.register(OpCode::PUSHDATA4, push_data);
    jump_table.register(OpCode::PUSHM1, push_m1);
    jump_table.register(OpCode::PUSH0, push_0);
    jump_table.register(OpCode::PUSH1, push_1);
    jump_table.register(OpCode::PUSH2, push_2);
    jump_table.register(OpCode::PUSH3, push_3);
    jump_table.register(OpCode::PUSH4, push_4);
    jump_table.register(OpCode::PUSH5, push_5);
    jump_table.register(OpCode::PUSH6, push_6);
    jump_table.register(OpCode::PUSH7, push_7);
    jump_table.register(OpCode::PUSH8, push_8);
    jump_table.register(OpCode::PUSH9, push_9);
    jump_table.register(OpCode::PUSH10, push_10);
    jump_table.register(OpCode::PUSH11, push_11);
    jump_table.register(OpCode::PUSH12, push_12);
    jump_table.register(OpCode::PUSH13, push_13);
    jump_table.register(OpCode::PUSH14, push_14);
    jump_table.register(OpCode::PUSH15, push_15);
    jump_table.register(OpCode::PUSH16, push_16);
    jump_table.register(OpCode::PUSHT, push_t);
    jump_table.register(OpCode::PUSHF, push_f);
}

/// Helper to get current context or return error.
#[inline]
fn require_context(
    engine: &mut ExecutionEngine,
) -> VmResult<&mut crate::execution_context::ExecutionContext> {
    engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))
}

/// Implements the PUSHINT8 operation.
fn push_int8(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let value = instruction.read_i8_operand()?;
    require_context(engine)?.push(StackItem::from_int(value))
}

/// Implements the PUSHINT16 operation.
fn push_int16(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let value = instruction.read_i16_operand()?;
    require_context(engine)?.push(StackItem::from_int(value))
}

/// Implements the PUSHINT32 operation.
fn push_int32(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let value = instruction.read_i32_operand()?;
    require_context(engine)?.push(StackItem::from_int(value))
}

/// Implements the PUSHINT64 operation.
fn push_int64(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let value = instruction.read_i64_operand()?;
    require_context(engine)?.push(StackItem::from_int(value))
}

/// Implements the PUSHINT128 operation.
fn push_int128(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let bytes = instruction.operand();
    if bytes.len() != 16 {
        return Err(VmError::invalid_instruction_msg(format!(
            "Expected 16 bytes for PUSHINT128, got {}",
            bytes.len()
        )));
    }
    let value = BigInt::from_signed_bytes_le(bytes);
    require_context(engine)?.push(StackItem::from_int(value))
}

/// Implements the PUSHINT256 operation.
fn push_int256(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let bytes = instruction.operand();
    if bytes.len() != HASH_SIZE {
        return Err(VmError::invalid_instruction_msg(format!(
            "Expected {} bytes for PUSHINT256, got {}",
            HASH_SIZE,
            bytes.len()
        )));
    }
    let value = BigInt::from_signed_bytes_le(bytes);
    require_context(engine)?.push(StackItem::from_int(value))
}

/// Implements the PUSHA operation.
fn push_a(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let context = require_context(engine)?;
    let offset = instruction.read_i32_operand()?;
    let current_ip = context.instruction_pointer();
    let address = current_ip as i32 + offset;
    let script_len = context.script().len();

    if address < 0 || address > script_len as i32 {
        return Err(VmError::invalid_operation_msg(format!(
            "Address out of bounds: {address}"
        )));
    }

    let script = context.script_arc();
    context.push(StackItem::from_pointer(script, address as usize))
}

/// Implements the PUSHNULL operation.
#[inline]
fn push_null(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    engine.push(StackItem::Null)
}

/// Unified PUSHDATA handler for PUSHDATA1, PUSHDATA2, PUSHDATA4.
/// The instruction already contains the parsed operand data.
fn push_data(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let data = instruction.operand();
    require_context(engine)?.push(StackItem::from_byte_string(data.to_vec()))
}

// Small integer push operations - all use the same pattern
#[inline]
fn push_m1(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_int(-1))
}
#[inline]
fn push_0(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_int(0))
}
#[inline]
fn push_1(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_int(1))
}
#[inline]
fn push_2(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_int(2))
}
#[inline]
fn push_3(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_int(3))
}
#[inline]
fn push_4(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_int(4))
}
#[inline]
fn push_5(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_int(5))
}
#[inline]
fn push_6(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_int(6))
}
#[inline]
fn push_7(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_int(7))
}
#[inline]
fn push_8(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_int(8))
}
#[inline]
fn push_9(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_int(9))
}
#[inline]
fn push_10(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_int(10))
}
#[inline]
fn push_11(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_int(11))
}
#[inline]
fn push_12(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_int(12))
}
#[inline]
fn push_13(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_int(13))
}
#[inline]
fn push_14(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_int(14))
}
#[inline]
fn push_15(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_int(15))
}
#[inline]
fn push_16(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_int(16))
}

/// Implements the PUSHT operation.
#[inline]
fn push_t(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_bool(true))
}

/// Implements the PUSHF operation.
#[inline]
fn push_f(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_bool(false))
}
