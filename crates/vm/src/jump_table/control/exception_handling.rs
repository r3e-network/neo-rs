//! Exception handling operations for the Neo Virtual Machine.

use crate::{
    execution_engine::ExecutionEngine,
    instruction::Instruction,
    Error,
};
use super::types::ExceptionHandler;

/// Implements the TRY operation.
pub fn try_op(engine: &mut ExecutionEngine, instruction: &Instruction) -> crate::Result<()> {
    // Get the current context
    let context = engine.current_context_mut().ok_or_else(|| Error::InvalidOperation("No current context".into()))?;

    // Get the catch and finally offsets from the instruction
    let catch_offset = instruction.read_i16_operand()?;
    let finally_offset = instruction.read_i16_operand()?;

    // Calculate absolute offsets
    let catch_absolute = if catch_offset != 0 {
        Some(context.instruction_pointer() + catch_offset as usize)
    } else {
        None
    };

    let finally_absolute = if finally_offset != 0 {
        Some(context.instruction_pointer() + finally_offset as usize)
    } else {
        None
    };

    // Create exception handler
    let handler = ExceptionHandler {
        catch_offset: catch_absolute,
        finally_offset: finally_absolute,
        stack_depth: context.evaluation_stack().len(),
    };

    // Push exception handler onto the exception handling stack
    context.push_exception_handler(handler);

    Ok(())
}

/// Implements the TRY_L operation (long try with 32-bit offsets).
pub fn try_l(engine: &mut ExecutionEngine, instruction: &Instruction) -> crate::Result<()> {
    // Get the current context
    let context = engine.current_context_mut().ok_or_else(|| Error::InvalidOperation("No current context".into()))?;

    // Get the catch and finally offsets from the instruction
    let catch_offset = instruction.read_i32_operand()?;
    let finally_offset = instruction.read_i32_operand()?;

    // Calculate absolute offsets
    let catch_absolute = if catch_offset != 0 {
        Some(context.instruction_pointer() + catch_offset as usize)
    } else {
        None
    };

    let finally_absolute = if finally_offset != 0 {
        Some(context.instruction_pointer() + finally_offset as usize)
    } else {
        None
    };

    // Create exception handler
    let handler = ExceptionHandler {
        catch_offset: catch_absolute,
        finally_offset: finally_absolute,
        stack_depth: context.evaluation_stack().len(),
    };

    // Push exception handler onto the exception handling stack
    context.push_exception_handler(handler);

    Ok(())
}

/// Implements the ENDTRY operation.
pub fn endtry(engine: &mut ExecutionEngine, instruction: &Instruction) -> crate::Result<()> {
    // Get the finally offset from the instruction
    let finally_offset = instruction.read_i16_operand()?;

    // Get the current context and pop the exception handler
    let (handler, should_jump, jump_target) = {
        let context = engine.current_context_mut().ok_or_else(|| Error::InvalidOperation("No current context".into()))?;
        
        let handler = context.pop_exception_handler()
            .ok_or_else(|| Error::InvalidOperation("No exception handler to end".into()))?;

        // Determine if we should jump and where
        let (should_jump, jump_target) = if finally_offset != 0 {
            let finally_absolute = context.instruction_pointer() + finally_offset as usize;
            context.set_instruction_pointer(finally_absolute);
            (true, finally_absolute)
        } else if let Some(finally_absolute) = handler.finally_offset {
            context.set_instruction_pointer(finally_absolute);
            (true, finally_absolute)
        } else {
            (false, 0)
        };

        (handler, should_jump, jump_target)
    };

    // Set jumping flag after releasing the context borrow
    if should_jump {
        engine.is_jumping = true;
    }

    Ok(())
}

/// Implements the ENDTRY_L operation (long endtry with 32-bit offset).
pub fn endtry_l(engine: &mut ExecutionEngine, instruction: &Instruction) -> crate::Result<()> {
    // Get the finally offset from the instruction
    let finally_offset = instruction.read_i32_operand()?;

    // Get the current context and pop the exception handler
    let (handler, should_jump, jump_target) = {
        let context = engine.current_context_mut().ok_or_else(|| Error::InvalidOperation("No current context".into()))?;
        
        let handler = context.pop_exception_handler()
            .ok_or_else(|| Error::InvalidOperation("No exception handler to end".into()))?;

        // Determine if we should jump and where
        let (should_jump, jump_target) = if finally_offset != 0 {
            let finally_absolute = context.instruction_pointer() + finally_offset as usize;
            context.set_instruction_pointer(finally_absolute);
            (true, finally_absolute)
        } else if let Some(finally_absolute) = handler.finally_offset {
            context.set_instruction_pointer(finally_absolute);
            (true, finally_absolute)
        } else {
            (false, 0)
        };

        (handler, should_jump, jump_target)
    };

    // Set jumping flag after releasing the context borrow
    if should_jump {
        engine.is_jumping = true;
    }

    Ok(())
}

/// Implements the ENDFINALLY operation.
pub fn endfinally(engine: &mut ExecutionEngine, _instruction: &Instruction) -> crate::Result<()> {
    // Get the current context
    let context = engine.current_context_mut().ok_or_else(|| Error::InvalidOperation("No current context".into()))?;

    // Check if we're in an exception state
    if context.is_in_exception() {
        // Re-throw the exception
        return Err(Error::ExecutionHalted("Exception re-thrown from finally block".into()));
    }

    // Continue normal execution
    Ok(())
}

/// Implements the THROW operation.
pub fn throw(engine: &mut ExecutionEngine, _instruction: &Instruction) -> crate::Result<()> {
    // Pop the exception message from the stack and handle exception processing
    let message = {
        let context = engine.current_context_mut().ok_or_else(|| Error::InvalidOperation("No current context".into()))?;
        let exception_message = context.pop()?.as_bytes()?;
        String::from_utf8(exception_message)
            .unwrap_or_else(|_| "Invalid exception message".to_string())
    };

    // Look for exception handlers
    loop {
        let handler_result = {
            let context = engine.current_context_mut().ok_or_else(|| Error::InvalidOperation("No current context".into()))?;
            context.pop_exception_handler()
        };

        if let Some(handler) = handler_result {
            // Restore stack to the depth when the handler was installed
            {
                let context = engine.current_context_mut().ok_or_else(|| Error::InvalidOperation("No current context".into()))?;
                while context.evaluation_stack().len() > handler.stack_depth {
                    context.pop()?;
                }
            }

            // If there's a catch block, jump to it
            if let Some(catch_offset) = handler.catch_offset {
                let context = engine.current_context_mut().ok_or_else(|| Error::InvalidOperation("No current context".into()))?;
                context.set_instruction_pointer(catch_offset);
                context.set_exception_state(true);
                engine.is_jumping = true;
                return Ok(());
            }

            // If there's a finally block, jump to it
            if let Some(finally_offset) = handler.finally_offset {
                let context = engine.current_context_mut().ok_or_else(|| Error::InvalidOperation("No current context".into()))?;
                context.set_instruction_pointer(finally_offset);
                context.set_exception_state(true);
                engine.is_jumping = true;
                return Ok(());
            }
        } else {
            // No more exception handlers
            break;
        }
    }

    // No exception handler found, halt execution
    engine.set_state(crate::execution_engine::VMState::FAULT);
    Err(Error::ExecutionHalted(format!("Unhandled exception: {}", message)))
}

/// Implements the ABORT operation.
pub fn abort(engine: &mut ExecutionEngine, _instruction: &Instruction) -> crate::Result<()> {
    // Set the engine state to FAULT
    engine.set_state(crate::execution_engine::VMState::FAULT);
    Err(Error::ExecutionHalted("Execution aborted".into()))
}

/// Implements the ASSERT operation.
pub fn assert(engine: &mut ExecutionEngine, _instruction: &Instruction) -> crate::Result<()> {
    // Get the current context
    let context = engine.current_context_mut().ok_or_else(|| Error::InvalidOperation("No current context".into()))?;

    // Pop the condition from the stack
    let condition = context.pop()?.as_bool()?;

    // If the condition is false, throw an assertion error
    if !condition {
        // Set the engine state to FAULT
        engine.set_state(crate::execution_engine::VMState::FAULT);
        return Err(Error::ExecutionHalted("Assertion failed".into()));
    }

    Ok(())
} 