//! Bitwise operations for the Neo Virtual Machine.

use crate::neo_vm::error::{VmError, VmResult};
use crate::neo_vm::execution_context::ExecutionContext;
use crate::neo_vm::execution_engine::ExecutionEngine;
use crate::neo_vm::jump_table::{register_jump_handlers, JumpTable};
use crate::neo_vm::stack_item::StackItem;
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
fn value_from_stack_item(item: StackItem) -> VmResult<StackValue> {
    match item {
        StackItem::Buffer(buffer) => Ok(StackValue::ByteString(buffer.data())),
        item => StackValue::try_from(item),
    }
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
    let value = value_from_stack_item(ctx.pop()?)?;
    let result = arithmetic::invert_value(value).map_err(semantics_error)?;
    push_stack_value(ctx, result)
}

fn binary_bitwise(
    engine: &mut ExecutionEngine,
    op: fn(StackValue, StackValue) -> Result<StackValue, String>,
) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let right = value_from_stack_item(ctx.pop()?)?;
    let left = value_from_stack_item(ctx.pop()?)?;
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
mod tests {
    use super::*;
    use crate::neo_vm::script::Script;
    use num_bigint::BigInt;

    fn engine_with_stack(items: Vec<StackItem>) -> ExecutionEngine {
        let mut engine = ExecutionEngine::new(None);
        engine
            .load_script(Script::new_relaxed(vec![OpCode::RET.byte()]), -1, 0)
            .expect("load test script");

        let ctx = engine.current_context_mut().expect("current context");
        for item in items {
            ctx.push(item).expect("push test item");
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
    fn and_accepts_buffer_via_byte_string_semantics() {
        let mut engine = engine_with_stack(vec![
            StackItem::from_buffer(vec![0xff]),
            StackItem::from_byte_string(vec![0x00, 0x80]),
        ]);

        and(&mut engine, &instruction(OpCode::AND)).expect("AND succeeds");

        assert_eq!(pop(&mut engine).as_int().unwrap(), BigInt::from(-32768));
    }

    #[test]
    fn xor_uses_signed_extension_for_mixed_width_operands() {
        let mut engine = engine_with_stack(vec![
            StackItem::from_byte_string(vec![0xff]),
            StackItem::from_byte_string(vec![0x00, 0x80]),
        ]);

        xor(&mut engine, &instruction(OpCode::XOR)).expect("XOR succeeds");

        assert_eq!(pop(&mut engine).as_int().unwrap(), BigInt::from(32767));
    }

    #[test]
    fn equal_does_not_coerce_buffer_to_byte_string() {
        let mut engine = engine_with_stack(vec![
            StackItem::from_buffer(vec![0x01]),
            StackItem::from_byte_string(vec![0x01]),
        ]);

        equal(&mut engine, &instruction(OpCode::EQUAL)).expect("EQUAL succeeds");

        assert!(!pop(&mut engine).as_bool().unwrap());
    }
}
