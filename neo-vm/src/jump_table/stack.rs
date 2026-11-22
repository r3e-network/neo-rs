//! Stack operations for the Neo Virtual Machine.
//!
//! This module provides the stack operation handlers for the Neo VM.

use crate::Instruction;
use crate::OpCode;
use crate::error::VmError;
use crate::error::VmResult;
use crate::execution_engine::ExecutionEngine;
use crate::jump_table::{JumpTable, register_jump_handlers};
use crate::stack_item::StackItem;
use num_bigint::Sign;
use num_traits::ToPrimitive;

/// Registers the stack operation handlers.
pub fn register_handlers(jump_table: &mut JumpTable) {
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
fn dup(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Peek the top item on the stack and push a copy
    let item = engine.peek(0)?;
    engine.push(item)?;
    Ok(())
}

/// Implements the SWAP operation.
#[inline]
fn swap(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Swap in-place — no pop/push, no reference counter churn.
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
    context.evaluation_stack_mut().swap(0, 1)
}

/// Implements the TUCK operation.
fn tuck(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

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
fn over(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

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
#[inline]
fn rot(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // v3.11.2: `EvaluationStack.Roll(2)` brings the third item to the top
    // without popping and pushing.
    engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?
        .evaluation_stack_mut()
        .roll(2)?;
    engine.collect_op_length(2);
    Ok(())
}

/// Implements the DEPTH operation.
fn depth(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Get the stack depth
    let depth = context.evaluation_stack().len();

    // Push the depth onto the stack
    context.push(StackItem::from_int(depth))?;

    Ok(())
}

/// Implements the DROP operation.
#[inline]
fn drop(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // v3.11.2 Drop collects the reference delta of the pop.
    let r = engine.reference_count();
    engine.pop()?;
    engine.collect_ref_delta(r - engine.reference_count());
    Ok(())
}

/// Implements the NIP operation.
fn nip(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // v3.11.2 Nip collects the reference delta of the removal.
    let r = engine.reference_count();
    {
        let context = engine
            .current_context_mut()
            .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

        if context.evaluation_stack().len() < 2 {
            return Err(VmError::stack_underflow_msg(0, 0));
        }

        context.evaluation_stack_mut().remove(1)?;
    }
    engine.collect_ref_delta(r - engine.reference_count());
    Ok(())
}

/// Implements the XDROP operation.
fn xdrop(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // XDROP: remove the item at index n from top. 2 RC ops instead of 2n+1.
    // v3.11.2 XDrop collects the reference delta and the index it removed at.
    let r = engine.reference_count();
    let n = {
        let context = engine
            .current_context_mut()
            .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

        let n = context
            .pop()?
            .into_int()?
            .to_usize()
            .ok_or_else(|| VmError::invalid_operation_msg("Invalid index"))?;

        if context.evaluation_stack().len() <= n {
            return Err(VmError::stack_underflow_msg(0, 0));
        }

        context.evaluation_stack_mut().remove(n)?;
        n
    };
    engine.collect_ref_delta(r - engine.reference_count());
    engine.collect_op_length(n as i64);
    Ok(())
}

/// Implements the CLEAR operation.
fn clear(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Clear the stack, collecting the released references and the old depth
    // the way v3.11.2 Clear does.
    let r = engine.reference_count();
    let length = {
        let context = engine
            .current_context_mut()
            .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
        let length = context.evaluation_stack().len() as i64;
        context.evaluation_stack_mut().clear();
        length
    };
    engine.collect_ref_delta(r - engine.reference_count());
    engine.collect_op_length(length);
    Ok(())
}

/// Implements the PICK operation.
fn pick(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the index from the stack
    let n = context
        .pop()?
        .into_int()?
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
fn roll(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // v3.11.2: `EvaluationStack.Roll(n)` moves the item in place without
    // popping and pushing; out-of-bounds faults come from Roll itself.
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    let n = context.pop()?.into_int()?;

    if n.sign() == Sign::Minus {
        let len = context.evaluation_stack().len();
        return Err(VmError::invalid_operation_msg(format!(
            "Out of stack bounds: {n}/{len}"
        )));
    }

    let n = n
        .to_usize()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid index"))?;

    context.evaluation_stack_mut().roll(n)?;
    engine.collect_op_length(n as i64);
    Ok(())
}

/// Implements the ROLL operation before neo-vm PR #575, the jump table every
/// height below `HF_Huyao` runs (C# `Roll_Before575`).
///
/// The pre-575 `Roll` popped `n` items first, so popping the single zero off
/// a one-item stack succeeded; the in-place v3.11.2 `Roll` faults on the same
/// input because the stack is empty once the index is popped. The old silent
/// behaviour is kept for that special case and everything else delegates to
/// the current implementation, which still collects the `RunStats` length.
pub(crate) fn roll_before_575(
    engine: &mut ExecutionEngine,
    instruction: &Instruction,
) -> VmResult<()> {
    let is_single_zero = {
        let context = engine
            .current_context_mut()
            .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
        let is_single_zero = context.evaluation_stack().len() == 1
            && context.peek(0)?.as_integer()?.sign() == Sign::NoSign;
        is_single_zero
    };
    if is_single_zero {
        engine.pop()?;
        return Ok(());
    }
    roll(engine, instruction)
}

/// Implements the REVERSE3 operation.
fn reverse3(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    context.evaluation_stack_mut().reverse(3)?;
    engine.collect_op_length(3);

    Ok(())
}

/// Implements the REVERSE4 operation.
fn reverse4(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    context.evaluation_stack_mut().reverse(4)?;
    engine.collect_op_length(4);

    Ok(())
}

/// Implements the REVERSEN operation.
fn reversen(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the count from the stack
    let n = context.pop()?.into_int()?;

    if n.sign() == Sign::Minus {
        return Err(VmError::invalid_operation_msg(format!(
            "Reverse count out of range: {n}"
        )));
    }

    let n = n
        .to_usize()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid count"))?;

    context.evaluation_stack_mut().reverse(n)?;
    engine.collect_op_length(n as i64);

    Ok(())
}

// TOALTSTACK and FROMALTSTACK removed - not in C# Neo
// These opcodes (0x4C and 0x4F) are not present in the C# implementation
