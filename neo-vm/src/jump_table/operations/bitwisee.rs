//! Bitwise operations for the Neo Virtual Machine.

use crate::error::VmResult;
use crate::execution_engine::ExecutionEngine;
use crate::jump_table::shared::push_integer;
use crate::jump_table::{JumpTable, get_integer, register_jump_handlers, require_context};
use crate::stack_item::StackItem;
use crate::{Instruction, OpCode};
use num_bigint::BigInt;

/// Registers the bitwise operation handlers.
pub fn register_handlers<S>(jump_table: &mut JumpTable<S>) {
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

fn invert<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let value = get_integer(ctx.pop()?)?;
    push_integer(ctx, !value, "integer overflow for INVERT")
}

fn binary_bitwise<S, F>(
    engine: &mut ExecutionEngine<S>,
    overflow_message: &'static str,
    op: F,
) -> VmResult<()>
where
    F: FnOnce(BigInt, BigInt) -> BigInt,
{
    let ctx = require_context(engine)?;
    let right = get_integer(ctx.pop()?)?;
    let left = get_integer(ctx.pop()?)?;
    push_integer(ctx, op(left, right), overflow_message)
}

fn and<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    binary_bitwise(engine, "integer overflow for AND", |left, right| {
        left & right
    })
}

fn or<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    binary_bitwise(engine, "integer overflow for OR", |left, right| {
        left | right
    })
}

fn xor<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    binary_bitwise(engine, "integer overflow for XOR", |left, right| {
        left ^ right
    })
}

fn equal<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    let (left, right) = {
        let ctx = require_context(engine)?;
        let right = ctx.pop()?;
        let left = ctx.pop()?;
        (left, right)
    };
    let result = left.equals_with_limits(&right, engine.limits())?;
    require_context(engine)?.push(StackItem::from_bool(result))
}

fn not_equal<S>(engine: &mut ExecutionEngine<S>, _: &Instruction) -> VmResult<()> {
    let (left, right) = {
        let ctx = require_context(engine)?;
        let right = ctx.pop()?;
        let left = ctx.pop()?;
        (left, right)
    };
    let result = !left.equals_with_limits(&right, engine.limits())?;
    require_context(engine)?.push(StackItem::from_bool(result))
}

#[cfg(test)]
#[path = "../../tests/jump_table/bitwisee.rs"]
mod tests;
