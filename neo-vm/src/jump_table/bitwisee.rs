//! Bitwise operations for the Neo Virtual Machine.

use crate::error::{VmError, VmResult};
use crate::execution_context::ExecutionContext;
use crate::execution_engine::ExecutionEngine;
use crate::jump_table::{JumpTable, numeric_operand, register_jump_handlers};
use crate::stack_item::StackItem;
use neo_vm_rs::semantics::arithmetic;
use neo_vm_rs::{Instruction, OpCode, StackValue};

#[inline]
fn require_context(engine: &mut ExecutionEngine) -> VmResult<&mut ExecutionContext> {
    engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))
}

#[inline]
fn semantics_error(error: String) -> VmError {
    VmError::invalid_operation_msg(error)
}

#[inline]
fn push_stack_value(ctx: &mut ExecutionContext, value: StackValue) -> VmResult<()> {
    ctx.push(StackItem::try_from(value)?)
}

/// Registers the bitwise operation handlers.
pub fn register_handlers(jump_table: &mut JumpTable) {
    register_jump_handlers![
        jump_table;
        OpCode::INVERT => invert,
        OpCode::AND => and,
        OpCode::OR => or,
        OpCode::XOR => xor,
        OpCode::EQUAL => equal,
        OpCode::NOTEQUAL => not_equal,
    ];
}

fn invert(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let value = numeric_operand(ctx.pop()?)?;
    let result = arithmetic::invert_value(value).map_err(semantics_error)?;
    push_stack_value(ctx, result)
}

fn binary_bitwise(
    engine: &mut ExecutionEngine,
    op: fn(StackValue, StackValue) -> Result<StackValue, String>,
) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let right = numeric_operand(ctx.pop()?)?;
    let left = numeric_operand(ctx.pop()?)?;
    let result = op(left, right).map_err(semantics_error)?;
    push_stack_value(ctx, result)
}

fn and(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    binary_bitwise(engine, arithmetic::bitwise_and_values)
}

fn or(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    binary_bitwise(engine, arithmetic::bitwise_or_values)
}

fn xor(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    binary_bitwise(engine, arithmetic::bitwise_xor_values)
}

fn equal(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let (left, right) = {
        let ctx = require_context(engine)?;
        if ctx.evaluation_stack().len() < 2 {
            return Err(VmError::insufficient_stack_items(
                2,
                ctx.evaluation_stack().len(),
            ));
        }
        (ctx.pop()?, ctx.pop()?)
    };
    let result = right.equals_with_limits(&left, engine.limits())?;
    require_context(engine)?.push(StackItem::from_bool(result))
}

fn not_equal(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let (left, right) = {
        let ctx = require_context(engine)?;
        (ctx.pop()?, ctx.pop()?)
    };
    let result = !right.equals_with_limits(&left, engine.limits())?;
    require_context(engine)?.push(StackItem::from_bool(result))
}

#[cfg(test)]
#[path = "../tests/jump_table/bitwisee.rs"]
mod tests;
