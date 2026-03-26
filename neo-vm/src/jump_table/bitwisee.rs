//! Bitwise operations for the Neo Virtual Machine.

use crate::error::VmError;
use crate::error::VmResult;
use crate::execution_context::ExecutionContext;
use crate::execution_engine::ExecutionEngine;
use crate::instruction::Instruction;
use crate::jump_table::JumpTable;
use crate::op_code::OpCode;
use crate::stack_item::StackItem;

/// Helper to get current context or return error.
#[inline]
fn require_context(engine: &mut ExecutionEngine) -> VmResult<&mut ExecutionContext> {
    engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))
}

/// Registers the bitwise operation handlers.
pub fn register_handlers(jump_table: &mut JumpTable) {
    jump_table.register(OpCode::INVERT, invert);
    jump_table.register(OpCode::AND, and);
    jump_table.register(OpCode::OR, or);
    jump_table.register(OpCode::XOR, xor);
    jump_table.register(OpCode::EQUAL, equal);
    jump_table.register(OpCode::NOTEQUAL, not_equal);
}

fn invert(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let x = ctx.pop()?.as_int()?;
    ctx.push(StackItem::from_int(!x))
}

/// Helper for binary bitwise operations (AND, OR, XOR)
fn binary_bitwise<F>(engine: &mut ExecutionEngine, int_op: F) -> VmResult<()>
where
    F: FnOnce(num_bigint::BigInt, num_bigint::BigInt) -> num_bigint::BigInt,
{
    let ctx = require_context(engine)?;
    let b = ctx.pop()?.as_int()?;
    let a = ctx.pop()?.as_int()?;
    ctx.push(StackItem::from_int(int_op(a, b)))
}

fn and(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    binary_bitwise(engine, |a, b| a & b)
}

fn or(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    binary_bitwise(engine, |a, b| a | b)
}

fn xor(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    binary_bitwise(engine, |a, b| a ^ b)
}

fn equal(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let (a, b) = {
        let ctx = require_context(engine)?;
        if ctx.evaluation_stack().len() < 2 {
            return Err(VmError::insufficient_stack_items(
                2,
                ctx.evaluation_stack().len(),
            ));
        }
        (ctx.pop()?, ctx.pop()?)
    };
    let result = b.equals_with_limits(&a, engine.limits())?;
    require_context(engine)?.push(StackItem::from_bool(result))
}

fn not_equal(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let (a, b) = {
        let ctx = require_context(engine)?;
        (ctx.pop()?, ctx.pop()?)
    };
    let result = !b.equals_with_limits(&a, engine.limits())?;
    require_context(engine)?.push(StackItem::from_bool(result))
}
