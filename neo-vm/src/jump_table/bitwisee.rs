//! Bitwise operations for the Neo Virtual Machine.

use crate::error::{VmError, VmResult};
use crate::execution_engine::ExecutionEngine;
use crate::jump_table::{
    JumpTable, numeric_operand, push_stack_value, register_jump_handlers, require_context,
    semantics_error,
};
use crate::stack_item::StackItem;
use neo_vm_rs::semantics::arithmetic;
use neo_vm_rs::{Instruction, OpCode, StackValue};

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
