use neo_vm::call_flags::CallFlags;
use neo_vm::error::VmResult;
use neo_vm::execution_engine::{ExecutionEngine, VMState};
use neo_vm::interop_service::InteropDescriptor;
use neo_vm::op_code::OpCode;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm::stack_item::StackItem;

fn push_one(engine: &mut ExecutionEngine) -> VmResult<()> {
    let context = engine
        .current_context_mut()
        .expect("script context should be available");
    context.evaluation_stack_mut().push(StackItem::from_int(1));
    Ok(())
}

#[test]
fn executes_simple_script_and_halts() {
    let mut engine = ExecutionEngine::new(None);

    let script = ScriptBuilder::new().emit_opcode(OpCode::RET).to_script();

    engine.load_script(script, -1, 0).expect("load_script");
    let state = engine.execute();

    assert_eq!(state, VMState::HALT);
    assert!(engine.invocation_stack().is_empty());
}

#[test]
fn executes_syscall_through_engine_service() {
    let mut engine = ExecutionEngine::new(None);

    {
        let service = engine
            .interop_service_mut()
            .expect("engine must have an interop service");
        service
            .register(InteropDescriptor {
                name: "Test.PushOne".to_string(),
                handler: Some(push_one),
                price: 0,
                required_call_flags: CallFlags::NONE,
            })
            .expect("register descriptor");
    }

    let script = ScriptBuilder::new()
        .emit_syscall("Test.PushOne")
        .expect("emit_syscall")
        .emit_opcode(OpCode::RET)
        .to_script();

    engine.load_script(script, -1, 0).expect("load_script");
    let state = engine.execute();

    assert_eq!(state, VMState::HALT);
    assert!(engine.invocation_stack().is_empty());

    let stack = engine.result_stack();
    assert!(stack.is_empty());

    // The pushed value should reside on the evaluation stack of the last context prior to RET.
    // Since RET moves items to the caller (result stack when root), we can assert via gas state by reloading script.
    // For simplicity, rerun the script step-by-step and inspect before RET.
    let mut engine = ExecutionEngine::new(None);
    {
        let service = engine
            .interop_service_mut()
            .expect("engine must have an interop service");
        service
            .register(InteropDescriptor {
                name: "Test.PushOne".to_string(),
                handler: Some(push_one),
                price: 0,
                required_call_flags: CallFlags::NONE,
            })
            .expect("register descriptor");
    }

    let script = ScriptBuilder::new()
        .emit_syscall("Test.PushOne")
        .expect("emit_syscall")
        .emit_opcode(OpCode::RET)
        .to_script();
    engine.load_script(script, -1, 0).expect("load_script");

    // Execute just the syscall instruction
    engine.execute_next().expect("execute syscall");
    {
        let context = engine.current_context().expect("context");
        let stack = context.evaluation_stack();
        assert_eq!(stack.len(), 1);
        assert_eq!(stack.peek(0).unwrap().as_int().unwrap(), 1.into());
    }
}
