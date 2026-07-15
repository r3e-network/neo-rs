//! Stack operations for the Neo Virtual Machine.
//!
//! This module provides the stack operation handlers for the Neo VM.

use crate::Instruction;
use crate::OpCode;
use crate::error::VmError;
use crate::error::VmResult;
use crate::execution_engine::ExecutionEngine;
use crate::jump_table::{JumpTable, register_jump_handlers, require_context};
use crate::stack_item::StackItem;
use num_bigint::Sign;
use num_traits::ToPrimitive;

/// Registers the stack operation handlers.
pub fn register_handlers<S>(jump_table: &mut JumpTable<S>) {
    register_jump_handlers![
        jump_table;
        OpCode::DUP => dup,
        OpCode::SWAP => swap,
        OpCode::TUCK => tuck,
        OpCode::OVER => over,
        OpCode::PICK => pick,
        OpCode::ROT => rot,
        OpCode::DEPTH => depth,
        OpCode::DROP => drop,
        OpCode::NIP => nip,
        OpCode::XDROP => xdrop,
        OpCode::CLEAR => clear,
        OpCode::ROLL => roll,
        OpCode::REVERSE3 => reverse3,
        OpCode::REVERSE4 => reverse4,
        OpCode::REVERSEN => reversen,
    ];
}

/// Implements the DUP operation.
#[inline]
fn dup<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    // Peek the top item on the stack and push a copy
    let item = engine.peek(0)?;
    engine.push(item)?;
    Ok(())
}

/// Implements the SWAP operation.
#[inline]
fn swap<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    // Swap in-place — no pop/push, no reference counter churn.
    let context = require_context(engine)?;
    context.evaluation_stack_mut().swap(0, 1)
}

/// Implements the TUCK operation.
fn tuck<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = require_context(engine)?;

    if context.evaluation_stack().len() < 2 {
        return Err(VmError::stack_underflow_msg(0, 0));
    }

    // Insert a copy of the top item before the second-to-top item.
    // Stack [... a b] -> [... b a b] using one insert instead of 2 pops + 3 pushes.
    let top_clone = context.peek(0)?.clone();
    context.insert(2, top_clone)?;

    Ok(())
}

/// Implements the OVER operation.
fn over<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = require_context(engine)?;

    if context.evaluation_stack().len() < 2 {
        return Err(VmError::stack_underflow_msg(0, 0));
    }

    // Peek the second item on the stack
    let item = context.peek(1)?;

    // Push a copy of the item onto the stack
    context.push(item)?;

    Ok(())
}

/// Implements the ROT operation.
fn rot<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    // ROT: [... a b c] → [... b c a]
    // Remove item at index 2 from top (a) and push to top.
    // 2 RC ops instead of 6.
    let context = require_context(engine)?;

    if context.evaluation_stack().len() < 3 {
        return Err(VmError::stack_underflow_msg(0, 0));
    }

    let item = context.evaluation_stack_mut().remove(2)?;
    context.push(item)
}

/// Implements the DEPTH operation.
fn depth<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = require_context(engine)?;

    // Get the stack depth
    let depth = context.evaluation_stack().len();

    // Push the depth onto the stack
    context.push(StackItem::from_int(depth))?;

    Ok(())
}

/// Implements the DROP operation.
#[inline]
fn drop<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context and pop the top item
    engine.pop()?;
    Ok(())
}

/// Implements the NIP operation.
fn nip<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    // NIP: remove the second-to-top item. 1 RC op instead of 3.
    let context = require_context(engine)?;

    if context.evaluation_stack().len() < 2 {
        return Err(VmError::stack_underflow_msg(0, 0));
    }

    context.evaluation_stack_mut().remove(1)?;
    Ok(())
}

/// Implements the XDROP operation.
fn xdrop<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    // XDROP: remove the item at index n from top. 2 RC ops instead of 2n+1.
    let context = require_context(engine)?;

    let n = super::get_integer(context.pop()?)?
        .to_usize()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid index"))?;

    if context.evaluation_stack().len() <= n {
        return Err(VmError::stack_underflow_msg(0, 0));
    }

    context.evaluation_stack_mut().remove(n)?;
    Ok(())
}

/// Implements the CLEAR operation.
fn clear<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = require_context(engine)?;

    // Clear the stack
    context.evaluation_stack_mut().clear();

    Ok(())
}

/// Implements the PICK operation.
fn pick<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = require_context(engine)?;

    // Pop the index from the stack
    let n = super::get_integer(context.pop()?)?
        .to_usize()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid index"))?;

    if context.evaluation_stack().len() <= n {
        return Err(VmError::stack_underflow_msg(0, 0));
    }

    // Peek the item at the specified index
    let item = context.peek(n)?;

    // Push a copy of the item onto the stack
    context.push(item)?;

    Ok(())
}

/// Implements the ROLL operation.
fn roll<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    // ROLL: remove the item at index n from top and push it to the top.
    // 3 RC ops instead of 2n+2.
    let context = require_context(engine)?;

    let n = super::get_integer(context.pop()?)?;

    if n.sign() == Sign::Minus {
        return Err(VmError::invalid_operation_msg(format!(
            "The negative value {n} is invalid for OpCode.ROLL"
        )));
    }

    let n = n
        .to_usize()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid index"))?;

    if n == 0 {
        return Ok(());
    }

    if context.evaluation_stack().len() <= n {
        return Err(VmError::stack_underflow_msg(0, 0));
    }

    let item = context.evaluation_stack_mut().remove(n)?;
    context.push(item)
}

/// Implements the REVERSE3 operation.
fn reverse3<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = require_context(engine)?;

    context.evaluation_stack_mut().reverse(3)?;

    Ok(())
}

/// Implements the REVERSE4 operation.
fn reverse4<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = require_context(engine)?;

    context.evaluation_stack_mut().reverse(4)?;

    Ok(())
}

/// Implements the REVERSEN operation.
fn reversen<S>(engine: &mut ExecutionEngine<S>, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = require_context(engine)?;

    // Pop the count from the stack
    let n = super::get_integer(context.pop()?)?;

    if n.sign() == Sign::Minus {
        return Err(VmError::invalid_operation_msg(format!(
            "Reverse count out of range: {n}"
        )));
    }

    let n = n
        .to_usize()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid count"))?;

    context.evaluation_stack_mut().reverse(n)?;

    Ok(())
}

// TOALTSTACK and FROMALTSTACK removed - not in C# Neo
// These opcodes (0x4C and 0x4F) are not present in the C# implementation
