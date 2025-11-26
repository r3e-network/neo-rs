//! Tests for the Neo VM exception handling opcodes after aligning with the
//! C# reference implementation.

use neo_vm::instruction::Instruction;
use neo_vm::jump_table::control::exception_handling as vm_try;
use neo_vm::op_code::OpCode;
use neo_vm::script::Script;
use neo_vm::stack_item::StackItem;
use neo_vm::{ExceptionHandlingContext, ExceptionHandlingState, ExecutionEngine, VMState};

fn make_script(bytes: &[u8]) -> Script {
    Script::new(bytes.to_vec(), false).expect("script should build")
}

#[test]
fn context_matches_reference_shape() {
    let mut ctx = ExceptionHandlingContext::new(12, 34);
    assert_eq!(ctx.catch_pointer(), 12);
    assert_eq!(ctx.finally_pointer(), 34);
    assert_eq!(ctx.end_pointer(), -1);
    assert!(ctx.has_catch());
    assert!(ctx.has_finally());
    assert_eq!(ctx.state(), ExceptionHandlingState::Try);

    ctx.set_end_pointer(99);
    assert_eq!(ctx.end_pointer(), 99);

    ctx.set_state(ExceptionHandlingState::Catch);
    assert_eq!(ctx.state(), ExceptionHandlingState::Catch);

    ctx.set_state(ExceptionHandlingState::Finally);
    assert_eq!(ctx.state(), ExceptionHandlingState::Finally);

    let no_handlers = ExceptionHandlingContext::new(-1, -1);
    assert!(!no_handlers.has_catch());
    assert!(!no_handlers.has_finally());
}

#[test]
fn try_and_endtry_push_and_update_context() {
    let mut engine = ExecutionEngine::new(None);
    let script = make_script(&[OpCode::NOP as u8]);
    engine.load_script(script, -1, 0).expect("context loads");

    let base_ip = engine
        .current_context()
        .expect("context available")
        .instruction_pointer();
    let try_instruction = Instruction::new(OpCode::TRY, &[2u8, 0u8, 3u8, 0u8]); // little-endian i16 offsets
    vm_try::try_op(&mut engine, &try_instruction).expect("try executes");

    let context = engine.current_context().expect("context available");
    let stack = context.try_stack().expect("try stack populated");
    assert_eq!(stack.len(), 1);
    assert_eq!(stack[0].catch_pointer(), base_ip as i32 + 2);
    assert_eq!(stack[0].finally_pointer(), base_ip as i32 + 3);

    let end_instruction = Instruction::new(OpCode::ENDTRY, &[4i8 as u8]);
    vm_try::endtry(&mut engine, &end_instruction).expect("endtry executes");

    let context = engine.current_context().expect("context available");
    let stack = context.try_stack().expect("try stack populated");
    assert_eq!(stack.len(), 1);
    assert_eq!(stack[0].state(), ExceptionHandlingState::Finally);
    assert_eq!(stack[0].end_pointer(), base_ip as i32 + 4);
}

#[test]
fn throw_routes_to_catch_block() {
    let mut engine = ExecutionEngine::new(None);
    let script = make_script(&[OpCode::NOP as u8; 8]); // simple valid script
    engine.load_script(script, -1, 0).expect("context loads");

    // Install TRY handler manually (catch at +2, no finally)
    let base_ip = engine
        .current_context()
        .expect("context available")
        .instruction_pointer();
    let try_instruction = Instruction::new(OpCode::TRY, &[2u8, 0u8, 0u8, 0u8]);
    vm_try::try_op(&mut engine, &try_instruction).expect("try executes");

    // Push an exception onto the evaluation stack and execute THROW
    engine
        .push(StackItem::from_byte_string(b"boom".to_vec()))
        .expect("push succeeds");
    vm_try::throw(&mut engine, &Instruction::new(OpCode::THROW, &[])).expect("throw handled");

    assert!(engine.uncaught_exception().is_none());
    assert_eq!(engine.state(), VMState::BREAK);

    let context = engine.current_context().expect("context available");
    assert_eq!(context.try_stack().unwrap().len(), 1);
    assert_eq!(context.instruction_pointer(), base_ip + 2);

    // Catch block should have received the exception on the evaluation stack
    assert_eq!(context.evaluation_stack().len(), 1);
    let value = context
        .evaluation_stack()
        .peek(0)
        .expect("value on stack")
        .as_bytes()
        .expect("byte string");
    assert_eq!(value, b"boom");
}

#[test]
fn throw_routes_to_finally_when_no_catch() {
    let mut engine = ExecutionEngine::new(None);
    let script = make_script(&[OpCode::NOP as u8; 8]); // simple valid script
    engine.load_script(script, -1, 0).expect("context loads");

    // TRY with no catch but with finally at +2
    let base_ip = engine
        .current_context()
        .expect("context available")
        .instruction_pointer();
    let try_instruction = Instruction::new(OpCode::TRY, &[0u8, 0u8, 2u8, 0u8]);
    vm_try::try_op(&mut engine, &try_instruction).expect("try executes");

    engine
        .push(StackItem::from_int(123))
        .expect("push succeeds");
    vm_try::throw(&mut engine, &Instruction::new(OpCode::THROW, &[])).expect("throw handled");

    // Exception is still recorded because we're heading into FINALLY
    assert!(engine.uncaught_exception().is_some());

    let context = engine.current_context().expect("context available");
    assert_eq!(context.instruction_pointer(), base_ip + 2);
    assert_eq!(engine.state(), VMState::BREAK);
}
