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
    let value = ctx.pop()?;
    let result = match value {
        StackItem::Integer(i) => StackItem::from_int(!i),
        StackItem::Boolean(b) => StackItem::from_bool(!b),
        _ => StackItem::from_bool(!value.as_bool()?),
    };
    ctx.push(result)
}

/// Helper for binary bitwise operations (AND, OR, XOR)
fn binary_bitwise<F, G>(engine: &mut ExecutionEngine, op_name: &str, int_op: F, bool_op: G) -> VmResult<()>
where
    F: FnOnce(num_bigint::BigInt, num_bigint::BigInt) -> num_bigint::BigInt,
    G: FnOnce(bool, bool) -> bool,
{
    let ctx = require_context(engine)?;
    let b = ctx.pop()?;
    let a = ctx.pop()?;

    let result = match (&a, &b) {
        (StackItem::Integer(a), StackItem::Integer(b)) => StackItem::from_int(int_op(a.clone(), b.clone())),
        (StackItem::Boolean(a), StackItem::Boolean(b)) => StackItem::from_bool(bool_op(*a, *b)),
        (StackItem::ByteString(_), StackItem::ByteString(_)) => {
            StackItem::from_int(int_op(a.as_int()?, b.as_int()?))
        }
        _ => {
            return Err(VmError::invalid_operation_msg(format!(
                "{} operation not supported for types: {:?} and {:?}",
                op_name, a.stack_item_type(), b.stack_item_type()
            )));
        }
    };
    ctx.push(result)
}

fn and(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    binary_bitwise(engine, "AND", |a, b| a & b, |a, b| a && b)
}

fn or(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    binary_bitwise(engine, "OR", |a, b| a | b, |a, b| a || b)
}

fn xor(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    binary_bitwise(engine, "XOR", |a, b| a ^ b, |a, b| a != b)
}

fn equal(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    let (a, b) = {
        let ctx = require_context(engine)?;
        if ctx.evaluation_stack().len() < 2 {
            return Err(VmError::insufficient_stack_items(2, ctx.evaluation_stack().len()));
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
