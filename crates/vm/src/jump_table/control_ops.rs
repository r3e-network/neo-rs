//! Control operations for the Neo Virtual Machine.
//!
//! This module provides the control operation handlers for the Neo VM,
//! following the exact structure of the C# Neo VM implementation.

use crate::{
    execution_engine::ExecutionEngine, instruction::Instruction, jump_table::JumpTable,
    op_code::OpCode, VmError, VmResult,
};
use num_traits::ToPrimitive;

/// Registers the control operation handlers.
pub fn register_handlers(jump_table: &mut JumpTable) {
    jump_table.register(OpCode::NOP, nop);
    jump_table.register(OpCode::JMP, jmp);
    jump_table.register(OpCode::JmpL, jmp_l);
    jump_table.register(OpCode::JMPIF, jmpif);
    jump_table.register(OpCode::JmpifL, jmpif_l);
    jump_table.register(OpCode::JMPIFNOT, jmpifnot);
    jump_table.register(OpCode::JmpifnotL, jmpifnot_l);
    jump_table.register(OpCode::JMPEQ, jmpeq);
    jump_table.register(OpCode::JmpeqL, jmpeq_l);
    jump_table.register(OpCode::JMPNE, jmpne);
    jump_table.register(OpCode::JmpneL, jmpne_l);
    jump_table.register(OpCode::JMPGT, jmpgt);
    jump_table.register(OpCode::JmpgtL, jmpgt_l);
    jump_table.register(OpCode::JMPGE, jmpge);
    jump_table.register(OpCode::JmpgeL, jmpge_l);
    jump_table.register(OpCode::JMPLT, jmplt);
    jump_table.register(OpCode::JmpltL, jmplt_l);
    jump_table.register(OpCode::JMPLE, jmple);
    jump_table.register(OpCode::JmpleL, jmple_l);
    jump_table.register(OpCode::CALL, call);
    jump_table.register(OpCode::CallL, call_l);
    jump_table.register(OpCode::CALLA, calla);
    jump_table.register(OpCode::CALLT, callt);
    jump_table.register(OpCode::ABORTMSG, abort_msg);
    jump_table.register(OpCode::ASSERTMSG, assert_msg);
    jump_table.register(OpCode::RET, ret);
}

/// Implements the NOP operation.
fn nop(_engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Do nothing
    Ok(())
}

/// Implements the JMP operation.
fn jmp(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Get the offset from the instruction
    let offset = instruction.read_i16_operand()?;

    // Calculate the new instruction pointer
    let new_ip = context.instruction_pointer() as i32 + offset as i32;
    if new_ip < 0 || new_ip > context.script().len() as i32 {
        return Err(VmError::invalid_operation_msg(format!(
            "Jump out of bounds: {new_ip}"
        )));
    }

    // Set the new instruction pointer
    context.set_instruction_pointer(new_ip as usize);

    // Set the jumping flag
    engine.is_jumping = true;

    Ok(())
}

/// Implements the JmpL operation.
fn jmp_l(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Get the offset from the instruction
    let offset = instruction.read_i32_operand()?;

    // Calculate the new instruction pointer
    let new_ip = context.instruction_pointer() as i32 + offset;
    if new_ip < 0 || new_ip > context.script().len() as i32 {
        return Err(VmError::invalid_operation_msg(format!(
            "Jump out of bounds: {new_ip}"
        )));
    }

    // Set the new instruction pointer
    context.set_instruction_pointer(new_ip as usize);

    // Set the jumping flag
    engine.is_jumping = true;

    Ok(())
}

/// Implements the JMPIF operation.
fn jmpif(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the condition from the stack
    let condition = context.pop()?.as_bool()?;

    if condition {
        // Get the offset from the instruction
        let offset = instruction.read_i16_operand()?;

        // Calculate the new instruction pointer
        let new_ip = context.instruction_pointer() as i32 + offset as i32;
        if new_ip < 0 || new_ip > context.script().len() as i32 {
            return Err(VmError::invalid_operation_msg(format!(
                "Jump out of bounds: {new_ip}"
            )));
        }

        // Set the new instruction pointer
        context.set_instruction_pointer(new_ip as usize);

        // Set the jumping flag
        engine.is_jumping = true;
    }

    Ok(())
}

/// Implements the JmpifL operation.
fn jmpif_l(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the condition from the stack
    let condition = context.pop()?.as_bool()?;

    if condition {
        // Get the offset from the instruction
        let offset = instruction.read_i32_operand()?;

        // Calculate the new instruction pointer
        let new_ip = context.instruction_pointer() as i32 + offset;
        if new_ip < 0 || new_ip > context.script().len() as i32 {
            return Err(VmError::invalid_operation_msg(format!(
                "Jump out of bounds: {new_ip}"
            )));
        }

        // Set the new instruction pointer
        context.set_instruction_pointer(new_ip as usize);

        // Set the jumping flag
        engine.is_jumping = true;
    }

    Ok(())
}

/// Implements the JMPIFNOT operation.
fn jmpifnot(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the condition from the stack
    let condition = context.pop()?.as_bool()?;

    if !condition {
        // Get the offset from the instruction
        let offset = instruction.read_i16_operand()?;

        // Calculate the new instruction pointer
        let new_ip = context.instruction_pointer() as i32 + offset as i32;
        if new_ip < 0 || new_ip > context.script().len() as i32 {
            return Err(VmError::invalid_operation_msg(format!(
                "Jump out of bounds: {new_ip}"
            )));
        }

        // Set the new instruction pointer
        context.set_instruction_pointer(new_ip as usize);

        // Set the jumping flag
        engine.is_jumping = true;
    }

    Ok(())
}

/// Implements the JmpifnotL operation.
fn jmpifnot_l(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the condition from the stack
    let condition = context.pop()?.as_bool()?;

    if !condition {
        // Get the offset from the instruction
        let offset = instruction.read_i32_operand()?;

        // Calculate the new instruction pointer
        let new_ip = context.instruction_pointer() as i32 + offset;
        if new_ip < 0 || new_ip > context.script().len() as i32 {
            return Err(VmError::invalid_operation_msg(format!(
                "Jump out of bounds: {new_ip}"
            )));
        }

        // Set the new instruction pointer
        context.set_instruction_pointer(new_ip as usize);

        // Set the jumping flag
        engine.is_jumping = true;
    }

    Ok(())
}

/// Implements the JMPEQ operation.
fn jmpeq(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the values from the stack
    let b = context.pop()?;
    let a = context.pop()?;

    if a.equals(&b)? {
        // Get the offset from the instruction
        let offset = instruction.read_i16_operand()?;

        // Calculate the new instruction pointer
        let new_ip = context.instruction_pointer() as i32 + offset as i32;
        if new_ip < 0 || new_ip > context.script().len() as i32 {
            return Err(VmError::invalid_operation_msg(format!(
                "Jump out of bounds: {new_ip}"
            )));
        }

        // Set the new instruction pointer
        context.set_instruction_pointer(new_ip as usize);

        // Set the jumping flag
        engine.is_jumping = true;
    }

    Ok(())
}

/// Implements the JmpeqL operation.
fn jmpeq_l(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the values from the stack
    let b = context.pop()?;
    let a = context.pop()?;

    if a.equals(&b)? {
        // Get the offset from the instruction
        let offset = instruction.read_i32_operand()?;

        // Calculate the new instruction pointer
        let new_ip = context.instruction_pointer() as i32 + offset;
        if new_ip < 0 || new_ip > context.script().len() as i32 {
            return Err(VmError::invalid_operation_msg(format!(
                "Jump out of bounds: {new_ip}"
            )));
        }

        // Set the new instruction pointer
        context.set_instruction_pointer(new_ip as usize);

        // Set the jumping flag
        engine.is_jumping = true;
    }

    Ok(())
}

/// Implements the JMPNE operation.
fn jmpne(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the values from the stack
    let b = context.pop()?;
    let a = context.pop()?;

    if !a.equals(&b)? {
        // Get the offset from the instruction
        let offset = instruction.read_i16_operand()?;

        // Calculate the new instruction pointer
        let new_ip = context.instruction_pointer() as i32 + offset as i32;
        if new_ip < 0 || new_ip > context.script().len() as i32 {
            return Err(VmError::invalid_operation_msg(format!(
                "Jump out of bounds: {new_ip}"
            )));
        }

        // Set the new instruction pointer
        context.set_instruction_pointer(new_ip as usize);

        // Set the jumping flag
        engine.is_jumping = true;
    }

    Ok(())
}

/// Implements the JmpneL operation.
fn jmpne_l(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the values from the stack
    let b = context.pop()?;
    let a = context.pop()?;

    if !a.equals(&b)? {
        // Get the offset from the instruction
        let offset = instruction.read_i32_operand()?;

        // Calculate the new instruction pointer
        let new_ip = context.instruction_pointer() as i32 + offset;
        if new_ip < 0 || new_ip > context.script().len() as i32 {
            return Err(VmError::invalid_operation_msg(format!(
                "Jump out of bounds: {new_ip}"
            )));
        }

        // Set the new instruction pointer
        context.set_instruction_pointer(new_ip as usize);

        // Set the jumping flag
        engine.is_jumping = true;
    }

    Ok(())
}

/// Implements the JMPGT operation.
fn jmpgt(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the values from the stack
    let b = context.pop()?.as_int()?;
    let a = context.pop()?.as_int()?;

    if a > b {
        // Get the offset from the instruction
        let offset = instruction.read_i16_operand()?;

        // Calculate the new instruction pointer
        let new_ip = context.instruction_pointer() as i32 + offset as i32;
        if new_ip < 0 || new_ip > context.script().len() as i32 {
            return Err(VmError::invalid_operation_msg(format!(
                "Jump out of bounds: {new_ip}"
            )));
        }

        // Set the new instruction pointer
        context.set_instruction_pointer(new_ip as usize);

        // Set the jumping flag
        engine.is_jumping = true;
    }

    Ok(())
}

/// Implements the JmpgtL operation.
fn jmpgt_l(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the values from the stack
    let b = context.pop()?.as_int()?;
    let a = context.pop()?.as_int()?;

    if a > b {
        // Get the offset from the instruction
        let offset = instruction.read_i32_operand()?;

        // Calculate the new instruction pointer
        let new_ip = context.instruction_pointer() as i32 + offset;
        if new_ip < 0 || new_ip > context.script().len() as i32 {
            return Err(VmError::invalid_operation_msg(format!(
                "Jump out of bounds: {new_ip}"
            )));
        }

        // Set the new instruction pointer
        context.set_instruction_pointer(new_ip as usize);

        // Set the jumping flag
        engine.is_jumping = true;
    }

    Ok(())
}

/// Implements the JMPGE operation.
fn jmpge(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the values from the stack
    let b = context.pop()?.as_int()?;
    let a = context.pop()?.as_int()?;

    if a >= b {
        // Get the offset from the instruction
        let offset = instruction.read_i16_operand()?;

        // Calculate the new instruction pointer
        let new_ip = context.instruction_pointer() as i32 + offset as i32;
        if new_ip < 0 || new_ip > context.script().len() as i32 {
            return Err(VmError::invalid_operation_msg(format!(
                "Jump out of bounds: {new_ip}"
            )));
        }

        // Set the new instruction pointer
        context.set_instruction_pointer(new_ip as usize);

        // Set the jumping flag
        engine.is_jumping = true;
    }

    Ok(())
}

/// Implements the JmpgeL operation.
fn jmpge_l(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the values from the stack
    let b = context.pop()?.as_int()?;
    let a = context.pop()?.as_int()?;

    if a >= b {
        // Get the offset from the instruction
        let offset = instruction.read_i32_operand()?;

        // Calculate the new instruction pointer
        let new_ip = context.instruction_pointer() as i32 + offset;
        if new_ip < 0 || new_ip > context.script().len() as i32 {
            return Err(VmError::invalid_operation_msg(format!(
                "Jump out of bounds: {new_ip}"
            )));
        }

        // Set the new instruction pointer
        context.set_instruction_pointer(new_ip as usize);

        // Set the jumping flag
        engine.is_jumping = true;
    }

    Ok(())
}

/// Implements the JMPLT operation.
fn jmplt(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the values from the stack
    let b = context.pop()?.as_int()?;
    let a = context.pop()?.as_int()?;

    if a < b {
        // Get the offset from the instruction
        let offset = instruction.read_i16_operand()?;

        // Calculate the new instruction pointer
        let new_ip = context.instruction_pointer() as i32 + offset as i32;
        if new_ip < 0 || new_ip > context.script().len() as i32 {
            return Err(VmError::invalid_operation_msg(format!(
                "Jump out of bounds: {new_ip}"
            )));
        }

        // Set the new instruction pointer
        context.set_instruction_pointer(new_ip as usize);

        // Set the jumping flag
        engine.is_jumping = true;
    }

    Ok(())
}

/// Implements the JmpltL operation.
fn jmplt_l(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the values from the stack
    let b = context.pop()?.as_int()?;
    let a = context.pop()?.as_int()?;

    if a < b {
        // Get the offset from the instruction
        let offset = instruction.read_i32_operand()?;

        // Calculate the new instruction pointer
        let new_ip = context.instruction_pointer() as i32 + offset;
        if new_ip < 0 || new_ip > context.script().len() as i32 {
            return Err(VmError::invalid_operation_msg(format!(
                "Jump out of bounds: {new_ip}"
            )));
        }

        // Set the new instruction pointer
        context.set_instruction_pointer(new_ip as usize);

        // Set the jumping flag
        engine.is_jumping = true;
    }

    Ok(())
}

/// Implements the JMPLE operation.
fn jmple(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the values from the stack
    let b = context.pop()?.as_int()?;
    let a = context.pop()?.as_int()?;

    if a <= b {
        // Get the offset from the instruction
        let offset = instruction.read_i16_operand()?;

        // Calculate the new instruction pointer
        let new_ip = context.instruction_pointer() as i32 + offset as i32;
        if new_ip < 0 || new_ip > context.script().len() as i32 {
            return Err(VmError::invalid_operation_msg(format!(
                "Jump out of bounds: {new_ip}"
            )));
        }

        // Set the new instruction pointer
        context.set_instruction_pointer(new_ip as usize);

        // Set the jumping flag
        engine.is_jumping = true;
    }

    Ok(())
}

/// Implements the JmpleL operation.
fn jmple_l(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the values from the stack
    let b = context.pop()?.as_int()?;
    let a = context.pop()?.as_int()?;

    if a <= b {
        // Get the offset from the instruction
        let offset = instruction.read_i32_operand()?;

        // Calculate the new instruction pointer
        let new_ip = context.instruction_pointer() as i32 + offset;
        if new_ip < 0 || new_ip > context.script().len() as i32 {
            return Err(VmError::invalid_operation_msg(format!(
                "Jump out of bounds: {new_ip}"
            )));
        }

        // Set the new instruction pointer
        context.set_instruction_pointer(new_ip as usize);

        // Set the jumping flag
        engine.is_jumping = true;
    }

    Ok(())
}

/// Implements the CALL operation.
fn call(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Get the offset from the instruction
    let offset = instruction.read_i16_operand()?;

    // Calculate the call target
    let call_target = context.instruction_pointer() as i32 + offset as i32;
    if call_target < 0 || call_target > context.script().len() as i32 {
        return Err(VmError::invalid_operation_msg(format!(
            "Call target out of bounds: {call_target}"
        )));
    }

    let script = context.script().clone();
    let new_context = engine.create_context(script, -1, call_target as usize);

    // Load the new context
    engine.load_context(new_context)?;

    // Set the jumping flag
    engine.is_jumping = true;

    Ok(())
}

/// Implements the CallL operation.
fn call_l(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Get the offset from the instruction
    let offset = instruction.read_i32_operand()?;

    // Calculate the call target
    let call_target = context.instruction_pointer() as i32 + offset;
    if call_target < 0 || call_target > context.script().len() as i32 {
        return Err(VmError::invalid_operation_msg(format!(
            "Call target out of bounds: {call_target}"
        )));
    }

    let script = context.script().clone();
    let new_context = engine.create_context(script, -1, call_target as usize);

    // Load the new context
    engine.load_context(new_context)?;

    // Set the jumping flag
    engine.is_jumping = true;

    Ok(())
}

/// Implements the CALLA operation.
fn calla(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the call target from the stack
    let call_target = context
        .pop()?
        .as_int()?
        .to_usize()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid call target"))?;

    let script = context.script().clone();
    let new_context = engine.create_context(script, -1, call_target);

    // Load the new context
    engine.load_context(new_context)?;

    // Set the jumping flag
    engine.is_jumping = true;

    Ok(())
}

/// Implements the CALLT operation.
fn callt(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    // Pop the call target from the stack
    let call_target = context
        .pop()?
        .as_int()?
        .to_usize()
        .ok_or_else(|| VmError::invalid_operation_msg("Invalid call target"))?;

    let script = context.script().clone();
    let new_context = engine.create_context(script, -1, call_target);

    // Load the new context
    engine.load_context(new_context)?;

    // Set the jumping flag
    engine.is_jumping = true;

    Ok(())
}

/// Implements the ABORTMSG operation.
/// This matches C# Neo's AbortMsg implementation exactly.
fn abort_msg(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    let message = context.pop()?;
    let message_bytes = message.as_bytes()?;
    let message_str = String::from_utf8_lossy(&message_bytes);

    log::error!("VM ABORT: {}", message_str);

    engine.set_state(crate::execution_engine::VMState::FAULT);

    Ok(())
}

/// Implements the ASSERTMSG operation.
/// This matches C# Neo's AssertMsg implementation exactly.
fn assert_msg(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    // Get the current context
    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    let message = context.pop()?;
    let condition = context.pop()?.as_bool()?;

    if !condition {
        let message_bytes = message.as_bytes()?;
        let message_str = String::from_utf8_lossy(&message_bytes);

        log::error!("VM ASSERT FAILED: {}", message_str);

        engine.set_state(crate::execution_engine::VMState::FAULT);
    }

    Ok(())
}

/// Implements the RET operation.
fn ret(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    if engine.invocation_stack().len() <= 1 {
        // No more contexts to return to - halt execution
        engine.set_state(crate::execution_engine::VMState::HALT);
        return Ok(());
    }

    let context_index = engine.invocation_stack().len() - 1;
    engine.remove_context(context_index)?;

    // The execution will continue in the previous context
    // Note: In C# Neo, the instruction pointer of the calling context
    // is automatically restored when the context is unloaded

    Ok(())
}
