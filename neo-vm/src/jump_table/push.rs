//! Push operations for the Neo Virtual Machine.
//!
//! This module provides the push operation handlers for the Neo VM.

use crate::error::VmError;
use crate::error::VmResult;
use crate::execution_engine::ExecutionEngine;
use crate::jump_table::{JumpTable, register_jump_handlers, require_context};
use crate::stack_item::StackItem;
use neo_vm_rs::Instruction;
use neo_vm_rs::OpCode;
use num_bigint::BigInt;

const HASH_SIZE: usize = 32;

/// Registers the push operation handlers.
pub fn register_handlers(jump_table: &mut JumpTable) {
    register_jump_handlers![
        jump_table;
        OpCode::PUSHINT8 => push_int8,
        OpCode::PUSHINT16 => push_int16,
        OpCode::PUSHINT32 => push_int32,
        OpCode::PUSHINT64 => push_int64,
        OpCode::PUSHINT128 => push_int128,
        OpCode::PUSHINT256 => push_int256,
        OpCode::PUSHA => push_a,
        OpCode::PUSHNULL => push_null,
        OpCode::PUSHDATA1 => push_data,
        OpCode::PUSHDATA2 => push_data,
        OpCode::PUSHDATA4 => push_data,
        OpCode::PUSHM1 => push_m1,
        OpCode::PUSH0 => push_0,
        OpCode::PUSH1 => push_1,
        OpCode::PUSH2 => push_2,
        OpCode::PUSH3 => push_3,
        OpCode::PUSH4 => push_4,
        OpCode::PUSH5 => push_5,
        OpCode::PUSH6 => push_6,
        OpCode::PUSH7 => push_7,
        OpCode::PUSH8 => push_8,
        OpCode::PUSH9 => push_9,
        OpCode::PUSH10 => push_10,
        OpCode::PUSH11 => push_11,
        OpCode::PUSH12 => push_12,
        OpCode::PUSH13 => push_13,
        OpCode::PUSH14 => push_14,
        OpCode::PUSH15 => push_15,
        OpCode::PUSH16 => push_16,
        OpCode::PUSHT => push_t,
        OpCode::PUSHF => push_f,
    ];
}

/// Implements the PUSHINT8 operation.
#[inline]
fn push_int8(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let value = instruction.read_i8_operand()?;
    require_context(engine)?.push(StackItem::from_int(value))
}

/// Implements the PUSHINT16 operation.
#[inline]
fn push_int16(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let value = instruction.read_i16_operand()?;
    require_context(engine)?.push(StackItem::from_int(value))
}

/// Implements the PUSHINT32 operation.
#[inline]
fn push_int32(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let value = instruction.read_i32_operand()?;
    require_context(engine)?.push(StackItem::from_int(value))
}

/// Implements the PUSHINT64 operation.
#[inline]
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
    // C# JumpTable.Push PushData1/2/4 each call engine.Limits.AssertMaxItemSize
    // on the operand length BEFORE pushing — a larger operand faults.
    let max_item_size = engine.limits().max_item_size as usize;
    if data.len() > max_item_size {
        return Err(VmError::invalid_operation_msg(format!(
            "MaxItemSize exceed: {}/{}",
            data.len(),
            max_item_size
        )));
    }
    require_context(engine)?.push(StackItem::from_byte_string(data.to_vec()))
}

// Small integer push operations - all use the same pattern
#[inline]
fn push_m1(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_i64(-1))
}
#[inline]
fn push_0(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_i64(0))
}
#[inline]
fn push_1(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_i64(1))
}
#[inline]
fn push_2(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_i64(2))
}
#[inline]
fn push_3(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_i64(3))
}
#[inline]
fn push_4(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_i64(4))
}
#[inline]
fn push_5(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_i64(5))
}
#[inline]
fn push_6(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_i64(6))
}
#[inline]
fn push_7(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_i64(7))
}
#[inline]
fn push_8(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_i64(8))
}
#[inline]
fn push_9(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_i64(9))
}
#[inline]
fn push_10(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_i64(10))
}
#[inline]
fn push_11(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_i64(11))
}
#[inline]
fn push_12(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_i64(12))
}
#[inline]
fn push_13(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_i64(13))
}
#[inline]
fn push_14(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_i64(14))
}
#[inline]
fn push_15(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_i64(15))
}
#[inline]
fn push_16(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_i64(16))
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
