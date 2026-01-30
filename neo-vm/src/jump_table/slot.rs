//! Slot operations for the Neo Virtual Machine.

use crate::error::VmError;
use crate::error::VmResult;
use crate::execution_context::ExecutionContext;
use crate::execution_engine::ExecutionEngine;
use crate::instruction::Instruction;
use crate::jump_table::JumpTable;
use crate::op_code::OpCode;

/// Helper to get current context or return error.
#[inline]
fn require_context(engine: &mut ExecutionEngine) -> VmResult<&mut ExecutionContext> {
    engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))
}

/// Registers the slot operation handlers.
pub fn register_handlers(jump_table: &mut JumpTable) {
    jump_table.register(OpCode::INITSSLOT, init_static_slot);
    jump_table.register(OpCode::INITSLOT, init_slot);

    // Static field operations (0-6 and generic)
    jump_table.register(OpCode::LDSFLD0, |e, _| load_static_field_n(e, 0));
    jump_table.register(OpCode::LDSFLD1, |e, _| load_static_field_n(e, 1));
    jump_table.register(OpCode::LDSFLD2, |e, _| load_static_field_n(e, 2));
    jump_table.register(OpCode::LDSFLD3, |e, _| load_static_field_n(e, 3));
    jump_table.register(OpCode::LDSFLD4, |e, _| load_static_field_n(e, 4));
    jump_table.register(OpCode::LDSFLD5, |e, _| load_static_field_n(e, 5));
    jump_table.register(OpCode::LDSFLD6, |e, _| load_static_field_n(e, 6));
    jump_table.register(OpCode::LDSFLD, load_static_field);
    jump_table.register(OpCode::STSFLD0, |e, _| store_static_field_n(e, 0));
    jump_table.register(OpCode::STSFLD1, |e, _| store_static_field_n(e, 1));
    jump_table.register(OpCode::STSFLD2, |e, _| store_static_field_n(e, 2));
    jump_table.register(OpCode::STSFLD3, |e, _| store_static_field_n(e, 3));
    jump_table.register(OpCode::STSFLD4, |e, _| store_static_field_n(e, 4));
    jump_table.register(OpCode::STSFLD5, |e, _| store_static_field_n(e, 5));
    jump_table.register(OpCode::STSFLD6, |e, _| store_static_field_n(e, 6));
    jump_table.register(OpCode::STSFLD, store_static_field);

    // Local variable operations (0-6 and generic)
    jump_table.register(OpCode::LDLOC0, |e, _| load_local_n(e, 0));
    jump_table.register(OpCode::LDLOC1, |e, _| load_local_n(e, 1));
    jump_table.register(OpCode::LDLOC2, |e, _| load_local_n(e, 2));
    jump_table.register(OpCode::LDLOC3, |e, _| load_local_n(e, 3));
    jump_table.register(OpCode::LDLOC4, |e, _| load_local_n(e, 4));
    jump_table.register(OpCode::LDLOC5, |e, _| load_local_n(e, 5));
    jump_table.register(OpCode::LDLOC6, |e, _| load_local_n(e, 6));
    jump_table.register(OpCode::LDLOC, load_local);
    jump_table.register(OpCode::STLOC0, |e, _| store_local_n(e, 0));
    jump_table.register(OpCode::STLOC1, |e, _| store_local_n(e, 1));
    jump_table.register(OpCode::STLOC2, |e, _| store_local_n(e, 2));
    jump_table.register(OpCode::STLOC3, |e, _| store_local_n(e, 3));
    jump_table.register(OpCode::STLOC4, |e, _| store_local_n(e, 4));
    jump_table.register(OpCode::STLOC5, |e, _| store_local_n(e, 5));
    jump_table.register(OpCode::STLOC6, |e, _| store_local_n(e, 6));
    jump_table.register(OpCode::STLOC, store_local);

    // Argument operations (0-6 and generic)
    jump_table.register(OpCode::LDARG0, |e, _| load_argument_n(e, 0));
    jump_table.register(OpCode::LDARG1, |e, _| load_argument_n(e, 1));
    jump_table.register(OpCode::LDARG2, |e, _| load_argument_n(e, 2));
    jump_table.register(OpCode::LDARG3, |e, _| load_argument_n(e, 3));
    jump_table.register(OpCode::LDARG4, |e, _| load_argument_n(e, 4));
    jump_table.register(OpCode::LDARG5, |e, _| load_argument_n(e, 5));
    jump_table.register(OpCode::LDARG6, |e, _| load_argument_n(e, 6));
    jump_table.register(OpCode::LDARG, load_argument);
    jump_table.register(OpCode::STARG0, |e, _| store_argument_n(e, 0));
    jump_table.register(OpCode::STARG1, |e, _| store_argument_n(e, 1));
    jump_table.register(OpCode::STARG2, |e, _| store_argument_n(e, 2));
    jump_table.register(OpCode::STARG3, |e, _| store_argument_n(e, 3));
    jump_table.register(OpCode::STARG4, |e, _| store_argument_n(e, 4));
    jump_table.register(OpCode::STARG5, |e, _| store_argument_n(e, 5));
    jump_table.register(OpCode::STARG6, |e, _| store_argument_n(e, 6));
    jump_table.register(OpCode::STARG, store_argument);
}

// ============================================================================
// Initialization Operations
// ============================================================================

fn init_slot(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;

    if ctx.local_variables().is_some() || ctx.arguments().is_some() {
        return Err(VmError::invalid_operation_msg("INITSLOT cannot be executed twice"));
    }

    let operand = instruction.operand();
    let local_count = *operand.first()
        .ok_or_else(|| VmError::invalid_instruction_msg("Missing local count"))? as usize;
    let argument_count = *operand.get(1)
        .ok_or_else(|| VmError::invalid_instruction_msg("Missing argument count"))? as usize;

    if local_count == 0 && argument_count == 0 {
        return Err(VmError::invalid_operation_msg("The operand is invalid for OpCode.INITSLOT"));
    }

    if local_count > 0 {
        let rc = ctx.evaluation_stack().reference_counter().clone();
        ctx.set_local_variables(Some(crate::slot::Slot::new(local_count, rc)));
    }

    if argument_count > 0 {
        let mut arg_items = Vec::with_capacity(argument_count);
        for _ in 0..argument_count {
            arg_items.push(ctx.pop()?);
        }
        let rc = ctx.evaluation_stack().reference_counter().clone();
        ctx.set_arguments(Some(crate::slot::Slot::with_items(arg_items, rc)));
    }

    Ok(())
}

fn init_static_slot(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let ctx = require_context(engine)?;

    let static_count = *instruction.operand().first()
        .ok_or_else(|| VmError::invalid_instruction_msg("Missing static count"))? as usize;

    if ctx.static_fields().is_some() {
        return Err(VmError::invalid_operation_msg("INITSSLOT cannot be executed twice"));
    }

    if static_count > 0 {
        let rc = ctx.evaluation_stack().reference_counter().clone();
        ctx.set_static_fields(Some(crate::slot::Slot::new(static_count, rc)));
    }

    Ok(())
}

// ============================================================================
// Static Field Operations
// ============================================================================

#[inline]
fn load_static_field_n(engine: &mut ExecutionEngine, index: usize) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let value = ctx.load_static_field(index)?;
    ctx.push(value)
}

#[inline]
fn store_static_field_n(engine: &mut ExecutionEngine, index: usize) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let value = ctx.pop()?;
    ctx.store_static_field(index, value)
}

fn load_static_field(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let index = *instruction.operand().first()
        .ok_or_else(|| VmError::invalid_instruction_msg("Missing index"))? as usize;
    load_static_field_n(engine, index)
}

fn store_static_field(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let index = *instruction.operand().first()
        .ok_or_else(|| VmError::invalid_instruction_msg("Missing index"))? as usize;
    store_static_field_n(engine, index)
}

// ============================================================================
// Local Variable Operations
// ============================================================================

#[inline]
fn load_local_n(engine: &mut ExecutionEngine, index: usize) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let value = ctx.load_local(index)?;
    ctx.push(value)
}

#[inline]
fn store_local_n(engine: &mut ExecutionEngine, index: usize) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let value = ctx.pop()?;
    ctx.store_local(index, value)
}

fn load_local(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let index = *instruction.operand().first()
        .ok_or_else(|| VmError::invalid_instruction_msg("Missing index"))? as usize;
    load_local_n(engine, index)
}

fn store_local(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let index = *instruction.operand().first()
        .ok_or_else(|| VmError::invalid_instruction_msg("Missing index"))? as usize;
    store_local_n(engine, index)
}

// ============================================================================
// Argument Operations
// ============================================================================

#[inline]
fn load_argument_n(engine: &mut ExecutionEngine, index: usize) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let value = ctx.load_argument(index)?;
    ctx.push(value)
}

#[inline]
fn store_argument_n(engine: &mut ExecutionEngine, index: usize) -> VmResult<()> {
    let ctx = require_context(engine)?;
    let value = ctx.pop()?;
    ctx.store_argument(index, value)
}

fn load_argument(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let index = *instruction.operand().first()
        .ok_or_else(|| VmError::invalid_instruction_msg("Missing index"))? as usize;
    load_argument_n(engine, index)
}

fn store_argument(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let index = *instruction.operand().first()
        .ok_or_else(|| VmError::invalid_instruction_msg("Missing index"))? as usize;
    store_argument_n(engine, index)
}
