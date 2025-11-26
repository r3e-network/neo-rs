//! Additional behavioural checks for the Neo VM exception handling port.

use neo_vm::instruction::Instruction;
use neo_vm::jump_table::control::exception_handling as vm_try;
use neo_vm::op_code::OpCode;
use neo_vm::script::Script;
use neo_vm::stack_item::StackItem;
use neo_vm::{ExceptionHandlingContext, ExceptionHandlingState, ExecutionEngine, VMState};

fn make_engine() -> ExecutionEngine {
    ExecutionEngine::new(None)
}

#[test]
fn context_reports_absent_handlers() {
    let ctx = ExceptionHandlingContext::new(-1, -1);
    assert!(!ctx.has_catch());
    assert!(!ctx.has_finally());
    assert_eq!(ctx.state(), ExceptionHandlingState::Try);
}

#[test]
fn throw_without_handler_faults_vm() {
    let mut engine = make_engine();
    let script = Script::new(vec![OpCode::THROW as u8], false).expect("script builds");
    engine.load_script(script, -1, 0).expect("context loads");
    engine.push(StackItem::from_int(1)).expect("push succeeds");

    let result = vm_try::throw(&mut engine, &Instruction::new(OpCode::THROW, &[]));
    assert!(result.is_err());
    assert_eq!(engine.state(), VMState::FAULT);
}

#[test]
fn endfinally_without_pending_exception_advances() {
    let mut engine = make_engine();
    let script = Script::new(vec![OpCode::ENDFINALLY as u8], false).expect("script builds");
    engine.load_script(script, -1, 0).expect("context loads");

    let context = engine.current_context_mut().expect("context");
    context.set_try_stack(Some(vec![ExceptionHandlingContext::new(5, -1)]));
    if let Some(stack) = context.try_stack_mut() {
        stack[0].set_end_pointer(7);
        stack[0].set_state(ExceptionHandlingState::Finally);
    }

    vm_try::endfinally(&mut engine, &Instruction::new(OpCode::ENDFINALLY, &[]))
        .expect("endfinally succeeds");

    assert_eq!(engine.current_context().unwrap().instruction_pointer(), 7);
    assert_eq!(engine.state(), VMState::BREAK);
}
