//! Exception handling opcodes translated from the C# reference implementation.

use crate::error::{VmError, VmResult};
use crate::exception_handling::{ExceptionHandlingContext, ExceptionHandlingState};
use crate::execution_engine::{ExecutionEngine, VMState};
use crate::instruction::Instruction;
use crate::stack_item::StackItem;

/// Executes the `TRY` opcode (8-bit offsets).
pub fn try_op(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let operand = instruction.operand();
    if operand.len() < 2 {
        return Err(VmError::invalid_operand_msg(
            "TRY expects two signed byte operands",
        ));
    }
    let catch_offset = operand[0] as i8 as i32;
    let finally_offset = operand[1] as i8 as i32;
    execute_try(engine, instruction, catch_offset, finally_offset)
}

/// Executes the `TRY_L` opcode (32-bit offsets).
pub fn try_l(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let operand = instruction.operand();
    if operand.len() < 8 {
        return Err(VmError::invalid_operand_msg(
            "TRY_L expects two 32-bit signed operands",
        ));
    }
    let catch_offset = i32::from_le_bytes([operand[0], operand[1], operand[2], operand[3]]);
    let finally_offset = i32::from_le_bytes([operand[4], operand[5], operand[6], operand[7]]);
    execute_try(engine, instruction, catch_offset, finally_offset)
}

/// Executes the `ENDTRY` opcode (8-bit offset).
pub fn endtry(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let operand = instruction.operand();
    if operand.is_empty() {
        return Err(VmError::invalid_operand_msg(
            "ENDTRY expects a signed byte operand",
        ));
    }
    let offset = operand[0] as i8 as i32;
    execute_end_try(engine, instruction, offset)
}

/// Executes the `ENDTRY_L` opcode (32-bit offset).
pub fn endtry_l(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let operand = instruction.operand();
    if operand.len() < 4 {
        return Err(VmError::invalid_operand_msg(
            "ENDTRY_L expects a 32-bit signed operand",
        ));
    }
    let offset = i32::from_le_bytes([operand[0], operand[1], operand[2], operand[3]]);
    execute_end_try(engine, instruction, offset)
}

/// Executes the `ENDFINALLY` opcode.
pub fn endfinally(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let handler = {
        let context = engine
            .current_context_mut()
            .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

        let try_stack = context.try_stack_mut().ok_or_else(|| {
            VmError::invalid_operation_msg("The corresponding TRY block cannot be found.")
        })?;

        try_stack.pop().ok_or_else(|| {
            VmError::invalid_operation_msg("The corresponding TRY block cannot be found.")
        })?
    };

    if engine.uncaught_exception().is_none() {
        if handler.end_pointer() < 0 {
            return Err(VmError::invalid_operation_msg(
                "ENDTRY was not executed before ENDFINALLY",
            ));
        }
        let target = handler.end_pointer() as usize;
        let context = engine
            .current_context_mut()
            .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
        context.set_instruction_pointer(target);
        engine.is_jumping = true;
        Ok(())
    } else {
        let pending = engine
            .uncaught_exception()
            .cloned()
            .expect("uncaught exception must be present");
        execute_throw(engine, Some(pending))
    }
}

/// Executes the `THROW` opcode.
pub fn throw(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let exception = engine.pop()?;
    execute_throw(engine, Some(exception))
}

/// Propagates an uncaught exception originating from within the runtime.
pub fn throw_uncaught(engine: &mut ExecutionEngine, exception: StackItem) -> VmResult<()> {
    execute_throw(engine, Some(exception))
}

/// Executes the `ABORT` opcode.
pub fn abort(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    engine.set_state(VMState::FAULT);
    Err(VmError::execution_halted_msg("Execution aborted"))
}

/// Executes the `ASSERT` opcode.
pub fn assert(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let condition = engine.pop()?.as_bool()?;
    if !condition {
        engine.set_state(VMState::FAULT);
        return Err(VmError::execution_halted_msg(
            "ASSERT executed with false result",
        ));
    }
    Ok(())
}

fn execute_try(
    engine: &mut ExecutionEngine,
    instruction: &Instruction,
    catch_offset: i32,
    finally_offset: i32,
) -> VmResult<()> {
    if catch_offset == 0 && finally_offset == 0 {
        return Err(VmError::invalid_operation_msg(
            "TRY requires either a catch or a finally target",
        ));
    }

    let max_try_depth = engine.limits().max_try_nesting_depth;

    let context = engine
        .current_context_mut()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

    let script_len = context.script().len();
    let base_pointer = context.instruction_pointer();

    let catch_pointer = if catch_offset == 0 {
        -1
    } else {
        compute_relative_pointer(base_pointer, catch_offset, script_len)?
    };

    let finally_pointer = if finally_offset == 0 {
        -1
    } else {
        compute_relative_pointer(base_pointer, finally_offset, script_len)?
    };

    if context.try_stack().is_none() {
        context.set_try_stack(Some(Vec::new()));
    }
    let try_stack = context
        .try_stack_mut()
        .expect("try stack initialised before push");

    if try_stack.len() as u32 >= max_try_depth {
        return Err(VmError::invalid_operation_msg("MaxTryNestingDepth exceed"));
    }

    try_stack.push(ExceptionHandlingContext::new(
        catch_pointer,
        finally_pointer,
    ));
    Ok(())
}

fn execute_end_try(
    engine: &mut ExecutionEngine,
    instruction: &Instruction,
    end_offset: i32,
) -> VmResult<()> {
    let (current_index, script_len) = {
        let context = engine
            .current_context()
            .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
        (context.instruction_pointer(), context.script().len())
    };

    let mut finally_target: Option<usize> = None;

    {
        let context = engine
            .current_context_mut()
            .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;

        let try_stack = context.try_stack_mut().ok_or_else(|| {
            VmError::invalid_operation_msg("The corresponding TRY block cannot be found.")
        })?;

        let handler = try_stack.last_mut().ok_or_else(|| {
            VmError::invalid_operation_msg("The corresponding TRY block cannot be found.")
        })?;

        if handler.state() == ExceptionHandlingState::Finally {
            return Err(VmError::invalid_operation_msg(
                "ENDTRY cannot be executed from within a FINALLY block",
            ));
        }

        let end_pointer = compute_relative_pointer(current_index, end_offset, script_len)?;

        if handler.has_finally() {
            handler.set_state(ExceptionHandlingState::Finally);
            handler.set_end_pointer(end_pointer);
            finally_target = Some(handler.finally_pointer() as usize);
        } else {
            try_stack.pop();
            context.set_instruction_pointer(end_pointer as usize);
            engine.is_jumping = true;
        }
    }

    if let Some(target) = finally_target {
        let context = engine
            .current_context_mut()
            .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
        context.set_instruction_pointer(target);
        engine.is_jumping = true;
    }

    Ok(())
}

fn execute_throw(engine: &mut ExecutionEngine, exception: Option<StackItem>) -> VmResult<()> {
    if let Some(ex) = exception {
        engine.set_uncaught_exception(Some(ex));
    } else if engine.uncaught_exception().is_none() {
        return Err(VmError::invalid_operation_msg(
            "THROW requires an exception value",
        ));
    }

    let mut index = engine.invocation_stack().len();
    while index > 0 {
        index -= 1;

        let action = {
            let stack = engine.invocation_stack_mut();
            let context = &mut stack[index];
            let try_stack_opt = context.try_stack_mut();
            if try_stack_opt.is_none() {
                None
            } else {
                let try_stack = try_stack_opt.unwrap();

                while let Some(handler) = try_stack.last() {
                    let should_pop = handler.state() == ExceptionHandlingState::Finally
                        || (handler.state() == ExceptionHandlingState::Catch
                            && !handler.has_finally());
                    if should_pop {
                        try_stack.pop();
                    } else {
                        break;
                    }
                }

                if let Some(handler) = try_stack.last_mut() {
                    if handler.state() == ExceptionHandlingState::Try && handler.has_catch() {
                        handler.set_state(ExceptionHandlingState::Catch);
                        Some(HandlerAction::Catch {
                            context_index: index,
                            target: handler.catch_pointer(),
                        })
                    } else if handler.has_finally() {
                        handler.set_state(ExceptionHandlingState::Finally);
                        Some(HandlerAction::Finally {
                            context_index: index,
                            target: handler.finally_pointer(),
                        })
                    } else {
                        try_stack.pop();
                        None
                    }
                } else {
                    None
                }
            }
        };

        if let Some(action) = action {
            // Pop frames above the handler context.
            let pop_count = engine.invocation_stack().len() - 1 - action.context_index();
            for _ in 0..pop_count {
                let top_index = engine.invocation_stack().len() - 1;
                let _ = engine.remove_context(top_index)?;
            }

            match action {
                HandlerAction::Catch {
                    target,
                    context_index,
                } => {
                    if let Some(exception) = engine.uncaught_exception().cloned() {
                        engine.push(exception)?;
                    }
                    engine.set_uncaught_exception(None);
                    let stack = engine.invocation_stack_mut();
                    let context = &mut stack[context_index];
                    context.set_instruction_pointer(target as usize);
                }
                HandlerAction::Finally {
                    target,
                    context_index,
                } => {
                    let stack = engine.invocation_stack_mut();
                    let context = &mut stack[context_index];
                    context.set_instruction_pointer(target as usize);
                }
            }

            engine.is_jumping = true;
            return Ok(());
        }
    }

    let message = engine
        .uncaught_exception()
        .map(|item| format!("{item:?}"))
        .unwrap_or_else(|| "<unknown exception>".to_string());
    Err(VmError::execution_halted_msg(format!(
        "Unhandled exception: {message}"
    )))
}

fn compute_relative_pointer(base: usize, offset: i32, script_len: usize) -> VmResult<i32> {
    let destination = base as i64 + offset as i64;
    if destination < 0 || destination > script_len as i64 {
        return Err(VmError::invalid_operation_msg(
            "Jump offset points outside of the script",
        ));
    }
    Ok(destination as i32)
}

#[derive(Debug, Clone, Copy)]
enum HandlerAction {
    Catch { context_index: usize, target: i32 },
    Finally { context_index: usize, target: i32 },
}

impl HandlerAction {
    fn context_index(self) -> usize {
        match self {
            HandlerAction::Catch { context_index, .. } => context_index,
            HandlerAction::Finally { context_index, .. } => context_index,
        }
    }
}
