//! `JumpTable` Control operations implementation matching C# Neo.VM.JumpTable.Control

use crate::error::{VmError, VmResult};
use crate::execution_engine::ExecutionEngine;
use crate::instruction::Instruction;
use crate::op_code::OpCode;
use crate::vm_state::VMState;

/// Register all control handlers
pub fn register_handlers(jump_table: &mut crate::jump_table::JumpTable) {
    use OpCode::{
        ABORT, ABORTMSG, ASSERT, ASSERTMSG, CALL, CALLA, CALLT, CALL_L, ENDFINALLY, ENDTRY,
        ENDTRY_L, JMP, JMPEQ, JMPEQ_L, JMPGE, JMPGE_L, JMPGT, JMPGT_L, JMPIF, JMPIFNOT, JMPIFNOT_L,
        JMPIF_L, JMPLE, JMPLE_L, JMPLT, JMPLT_L, JMPNE, JMPNE_L, JMP_L, NOP, RET, SYSCALL, THROW,
        TRY, TRY_L,
    };

    jump_table.register(NOP, nop);
    jump_table.register(JMP, jmp);
    jump_table.register(JMP_L, jmp_l);
    jump_table.register(JMPIF, jmpif);
    jump_table.register(JMPIF_L, jmpif_l);
    jump_table.register(JMPIFNOT, jmpifnot);
    jump_table.register(JMPIFNOT_L, jmpifnot_l);
    jump_table.register(JMPEQ, jmpeq);
    jump_table.register(JMPEQ_L, jmpeq_l);
    jump_table.register(JMPNE, jmpne);
    jump_table.register(JMPNE_L, jmpne_l);
    jump_table.register(JMPGT, jmpgt);
    jump_table.register(JMPGT_L, jmpgt_l);
    jump_table.register(JMPGE, jmpge);
    jump_table.register(JMPGE_L, jmpge_l);
    jump_table.register(JMPLT, jmplt);
    jump_table.register(JMPLT_L, jmplt_l);
    jump_table.register(JMPLE, jmple);
    jump_table.register(JMPLE_L, jmple_l);
    jump_table.register(CALL, call);
    jump_table.register(CALL_L, call_l);
    jump_table.register(CALLA, calla);
    jump_table.register(CALLT, callt);
    jump_table.register(ABORT, abort);
    jump_table.register(ABORTMSG, abortmsg);
    jump_table.register(ASSERT, assert);
    jump_table.register(ASSERTMSG, assertmsg);
    jump_table.register(THROW, throw);
    jump_table.register(TRY, r#try);
    jump_table.register(TRY_L, try_l);
    jump_table.register(ENDTRY, endtry);
    jump_table.register(ENDTRY_L, endtry_l);
    jump_table.register(ENDFINALLY, endfinally);
    jump_table.register(RET, ret);
    jump_table.register(SYSCALL, syscall);
}

/// NOP - No operation
pub fn nop(_engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    Ok(())
}

/// JMP - Jump with signed byte offset
pub fn jmp(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let offset = i32::from(instruction.token_i8());
    engine.execute_jump_offset(offset)
}

/// `JMP_L` - Jump with 32-bit offset
pub fn jmp_l(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let offset = instruction.token_i32();
    engine.execute_jump_offset(offset)
}

/// JMPIF - Jump if true
pub fn jmpif(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    if engine.pop()?.get_boolean()? {
        let offset = i32::from(instruction.token_i8());
        engine.execute_jump_offset(offset)?;
    }
    Ok(())
}

/// `JMPIF_L` - Jump if true (32-bit)
pub fn jmpif_l(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    if engine.pop()?.get_boolean()? {
        let offset = instruction.token_i32();
        engine.execute_jump_offset(offset)?;
    }
    Ok(())
}

/// JMPIFNOT - Jump if false
pub fn jmpifnot(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    if !engine.pop()?.get_boolean()? {
        let offset = i32::from(instruction.token_i8());
        engine.execute_jump_offset(offset)?;
    }
    Ok(())
}

/// `JMPIFNOT_L` - Jump if false (32-bit)
pub fn jmpifnot_l(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    if !engine.pop()?.get_boolean()? {
        let offset = instruction.token_i32();
        engine.execute_jump_offset(offset)?;
    }
    Ok(())
}

/// JMPEQ - Jump if equal
pub fn jmpeq(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let b = engine.pop()?;
    let a = engine.pop()?;
    if a.equals_with_limits(&b, engine.limits())? {
        let offset = i32::from(instruction.token_i8());
        engine.execute_jump_offset(offset)?;
    }
    Ok(())
}

/// `JMPEQ_L` - Jump if equal (32-bit)
pub fn jmpeq_l(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let b = engine.pop()?;
    let a = engine.pop()?;
    if a.equals_with_limits(&b, engine.limits())? {
        let offset = instruction.token_i32();
        engine.execute_jump_offset(offset)?;
    }
    Ok(())
}

/// JMPNE - Jump if not equal
pub fn jmpne(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let b = engine.pop()?;
    let a = engine.pop()?;
    if !a.equals_with_limits(&b, engine.limits())? {
        let offset = i32::from(instruction.token_i8());
        engine.execute_jump_offset(offset)?;
    }
    Ok(())
}

/// `JMPNE_L` - Jump if not equal (32-bit)
pub fn jmpne_l(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let b = engine.pop()?;
    let a = engine.pop()?;
    if !a.equals_with_limits(&b, engine.limits())? {
        let offset = instruction.token_i32();
        engine.execute_jump_offset(offset)?;
    }
    Ok(())
}

/// JMPGT - Jump if greater than
pub fn jmpgt(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let b = engine.pop()?;
    let a = engine.pop()?;
    let a_int = a.get_integer()?;
    let b_int = b.get_integer()?;
    if a_int > b_int {
        let offset = i32::from(instruction.token_i8());
        engine.execute_jump_offset(offset)?;
    }
    Ok(())
}

/// `JMPGT_L` - Jump if greater than (32-bit)
pub fn jmpgt_l(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let b = engine.pop()?;
    let a = engine.pop()?;
    let a_int = a.get_integer()?;
    let b_int = b.get_integer()?;
    if a_int > b_int {
        let offset = instruction.token_i32();
        engine.execute_jump_offset(offset)?;
    }
    Ok(())
}

/// JMPGE - Jump if greater or equal
pub fn jmpge(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let b = engine.pop()?;
    let a = engine.pop()?;
    let a_int = a.get_integer()?;
    let b_int = b.get_integer()?;
    if a_int >= b_int {
        let offset = i32::from(instruction.token_i8());
        engine.execute_jump_offset(offset)?;
    }
    Ok(())
}

/// `JMPGE_L` - Jump if greater or equal (32-bit)
pub fn jmpge_l(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let b = engine.pop()?;
    let a = engine.pop()?;
    let a_int = a.get_integer()?;
    let b_int = b.get_integer()?;
    if a_int >= b_int {
        let offset = instruction.token_i32();
        engine.execute_jump_offset(offset)?;
    }
    Ok(())
}

/// JMPLT - Jump if less than
pub fn jmplt(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let b = engine.pop()?;
    let a = engine.pop()?;
    let a_int = a.get_integer()?;
    let b_int = b.get_integer()?;
    if a_int < b_int {
        let offset = i32::from(instruction.token_i8());
        engine.execute_jump_offset(offset)?;
    }
    Ok(())
}

/// `JMPLT_L` - Jump if less than (32-bit)
pub fn jmplt_l(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let b = engine.pop()?;
    let a = engine.pop()?;
    let a_int = a.get_integer()?;
    let b_int = b.get_integer()?;
    if a_int < b_int {
        let offset = instruction.token_i32();
        engine.execute_jump_offset(offset)?;
    }
    Ok(())
}

/// JMPLE - Jump if less or equal
pub fn jmple(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let b = engine.pop()?;
    let a = engine.pop()?;
    let a_int = a.get_integer()?;
    let b_int = b.get_integer()?;
    if a_int <= b_int {
        let offset = i32::from(instruction.token_i8());
        engine.execute_jump_offset(offset)?;
    }
    Ok(())
}

/// `JMPLE_L` - Jump if less or equal (32-bit)
pub fn jmple_l(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let b = engine.pop()?;
    let a = engine.pop()?;
    let a_int = a.get_integer()?;
    let b_int = b.get_integer()?;
    if a_int <= b_int {
        let offset = instruction.token_i32();
        engine.execute_jump_offset(offset)?;
    }
    Ok(())
}

/// CALL - Call function
pub fn call(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let offset = instruction.token_i8() as isize;
    let context = engine
        .current_context()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
    let position = context
        .instruction_pointer()
        .checked_add_signed(offset)
        .ok_or_else(|| VmError::InvalidJump(offset as i32))?;
    engine.execute_call(position)
}

/// `CALL_L` - Call function (32-bit)
pub fn call_l(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let offset = instruction.token_i32() as isize;
    let context = engine
        .current_context()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
    let position = context
        .instruction_pointer()
        .checked_add_signed(offset)
        .ok_or_else(|| VmError::InvalidJump(offset as i32))?;
    engine.execute_call(position)
}

/// CALLA - Call function at address from stack
pub fn calla(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let pointer = engine.pop()?.get_pointer()?;
    let current_context = engine
        .current_context()
        .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
    let pointer_script = pointer.script_arc();
    let current_script = current_context.script_arc();
    if !std::sync::Arc::ptr_eq(&pointer_script, &current_script) {
        return Err(VmError::invalid_operation_msg(
            "Pointers can't be shared between scripts".to_string(),
        ));
    }
    engine.execute_call(pointer.position())
}

/// CALLT - Call function with token
///
/// This opcode delegates to the `InteropHost`'s `on_callt` method, which is expected
/// to be implemented by `ApplicationEngine` to resolve method tokens and perform
/// cross-contract calls.
pub fn callt(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let token_id = instruction.token_u16();
    engine.invoke_callt(token_id)
}

/// ABORT - Abort execution
pub fn abort(_engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    Err(VmError::Abort)
}

/// ABORTMSG - Abort execution with message
pub fn abortmsg(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let _ = engine.pop();
    abort(engine, instruction)
}

/// ASSERT - Assert condition
pub fn assert(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    if !engine.pop()?.get_boolean()? {
        return Err(VmError::AssertFailed);
    }
    Ok(())
}

/// ASSERTMSG - Assert condition with message
pub fn assertmsg(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let _ = engine.pop();
    assert(engine, instruction)
}

/// THROW - Throw exception
pub fn throw(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    let ex = engine.pop()?;
    engine.execute_throw(Some(ex))
}

/// TRY - Begin try block
pub fn r#try(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let operand = instruction.operand();
    let catch_offset = i32::from(i16::from_le_bytes([
        *operand.first().unwrap_or(&0),
        *operand.get(1).unwrap_or(&0),
    ]));
    let finally_offset = i32::from(i16::from_le_bytes([
        *operand.get(2).unwrap_or(&0),
        *operand.get(3).unwrap_or(&0),
    ]));
    engine.execute_try(catch_offset, finally_offset)
}

/// `TRY_L` - Begin try block (32-bit)
pub fn try_l(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let catch_offset = instruction.token_i32();
    let finally_offset = instruction.token_i32_1();
    engine.execute_try(catch_offset, finally_offset)
}

/// ENDTRY - End try block
pub fn endtry(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let end_offset = i32::from(instruction.token_i8());
    engine.execute_end_try(end_offset)
}

/// `ENDTRY_L` - End try block (32-bit)
pub fn endtry_l(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let end_offset = instruction.token_i32();
    engine.execute_end_try(end_offset)
}

/// ENDFINALLY - End finally block
pub fn endfinally(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    engine.execute_end_finally()
}

/// RET - Return from function
pub fn ret(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    if engine.invocation_stack().is_empty() {
        engine.set_state(VMState::HALT);
        return Ok(());
    }

    let context_index = engine.invocation_stack().len() - 1;
    let mut context = engine.invocation_stack_mut().remove(context_index);

    let rvcount = context.rvcount();
    #[cfg(debug_assertions)]
    println!(
        "RET handler: rvcount={}, eval_stack_len={}",
        rvcount,
        context.evaluation_stack().len()
    );
    if rvcount != 0 {
        let eval_stack_len = context.evaluation_stack().len();
        let capacity = if rvcount == -1 {
            eval_stack_len
        } else {
            (rvcount as usize).min(eval_stack_len)
        };
        let mut items = Vec::with_capacity(capacity);

        if rvcount == -1 {
            for i in 0..eval_stack_len {
                if let Ok(item) = context.evaluation_stack().peek(i) {
                    items.push(item.clone());
                }
            }
        } else if rvcount > 0 {
            let count = (rvcount as usize).min(eval_stack_len);
            for i in 0..count {
                if let Ok(item) = context.evaluation_stack().peek(i) {
                    items.push(item.clone());
                }
            }
        }

        let return_to_result_stack = engine.invocation_stack().is_empty();
        items.reverse();
        if return_to_result_stack {
            for item in items {
                engine.result_stack_mut().push(item)?;
            }
        } else {
            let caller = engine
                .current_context_mut()
                .ok_or_else(|| VmError::invalid_operation_msg("No caller context"))?;
            for item in items {
                caller.push(item)?;
            }
        }
    }

    engine.unload_context(&mut context)?;

    if engine.invocation_stack().is_empty() {
        engine.set_state(VMState::HALT);
    }

    Ok(())
}

/// SYSCALL - System call
pub fn syscall(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
    let descriptor = instruction.token_u32();
    engine.on_syscall(descriptor)
}

/// Compatibility module exposing the exception-handling ops with explicit names,
/// matching the helper layout used in the C# test suite.
pub mod exception_handling {
    use crate::error::VmResult;
    use crate::execution_engine::ExecutionEngine;
    use crate::instruction::Instruction;

    /// Executes the TRY opcode for exception handling.
    pub fn try_op(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
        super::r#try(engine, instruction)
    }

    /// Executes the TRY_L opcode (long form) for exception handling.
    pub fn try_l(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
        super::try_l(engine, instruction)
    }

    /// Executes the ENDTRY opcode to end a try block.
    pub fn endtry(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
        super::endtry(engine, instruction)
    }

    /// Executes the ENDTRY_L opcode (long form) to end a try block.
    pub fn endtry_l(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
        super::endtry_l(engine, instruction)
    }

    /// Executes the ENDFINALLY opcode to end a finally block.
    pub fn endfinally(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
        super::endfinally(engine, instruction)
    }

    /// Executes the THROW opcode to throw an exception.
    pub fn throw(engine: &mut ExecutionEngine, instruction: &Instruction) -> VmResult<()> {
        super::throw(engine, instruction)
    }
}
