//! Bitwise operations for the Neo Virtual Machine.

use crate::neo_vm::error::VmError;
use crate::neo_vm::error::VmResult;
use crate::neo_vm::execution_context::ExecutionContext;
use crate::neo_vm::execution_engine::ExecutionEngine;
use crate::neo_vm::jump_table::JumpTable;
use crate::neo_vm::stack_item::StackItem;
use neo_vm_rs::Instruction;
use neo_vm_rs::OpCode;
use num_traits::ToPrimitive;

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
    let x = ctx.pop()?.into_int()?;
    let result = if let Some(x) = x.to_i64() {
        StackItem::from_int(neo_vm_rs::semantics::arithmetic::bitwise_not_i64(x))
    } else {
        StackItem::from_int(!x)
    };
    ctx.push(result)
}

/// Helper for binary bitwise operations (AND, OR, XOR)
fn binary_bitwise<BigIntOp, I64Op>(
    engine: &mut ExecutionEngine,
    bigint_op: BigIntOp,
    i64_op: I64Op,
) -> VmResult<()>
where
    BigIntOp: FnOnce(num_bigint::BigInt, num_bigint::BigInt) -> num_bigint::BigInt,
    I64Op: FnOnce(i64, i64) -> i64,
{
    let ctx = require_context(engine)?;
    let b = ctx.pop()?.into_int()?;
    let a = ctx.pop()?.into_int()?;
    let result = match (a.to_i64(), b.to_i64()) {
        (Some(a), Some(b)) => StackItem::from_int(i64_op(a, b)),
        _ => StackItem::from_int(bigint_op(a, b)),
    };
    ctx.push(result)
}

fn and(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    binary_bitwise(
        engine,
        |a, b| a & b,
        neo_vm_rs::semantics::arithmetic::bitwise_and_i64,
    )
}

fn or(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    binary_bitwise(
        engine,
        |a, b| a | b,
        neo_vm_rs::semantics::arithmetic::bitwise_or_i64,
    )
}

fn xor(engine: &mut ExecutionEngine, _: &Instruction) -> VmResult<()> {
    binary_bitwise(
        engine,
        |a, b| a ^ b,
        neo_vm_rs::semantics::arithmetic::bitwise_xor_i64,
    )
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
