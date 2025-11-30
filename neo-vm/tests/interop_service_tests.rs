use neo_vm::call_flags::CallFlags;
use neo_vm::error::VmError;
use neo_vm::error::VmResult;
use neo_vm::execution_engine::ExecutionEngine;
use neo_vm::interop_service::{InteropHost, InteropService, VmInteropDescriptor};
use neo_vm::op_code::OpCode;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm::stack_item::StackItem;

fn push_constant_handler(engine: &mut ExecutionEngine) -> VmResult<()> {
    let context = engine
        .current_context_mut()
        .expect("script context must exist for handler");
    context.evaluation_stack_mut().push(StackItem::from_int(42));
    Ok(())
}

#[test]
fn registers_and_fetches_descriptor() {
    let mut service = InteropService::new();

    let hash = service
        .register(VmInteropDescriptor {
            name: "Test.Method".to_string(),
            handler: Some(push_constant_handler),
            price: 10,
            required_call_flags: CallFlags::NONE,
        })
        .expect("registration should succeed");

    assert_eq!(service.len(), 1);
    assert!(!service.is_empty());

    let descriptor = service
        .get_method(b"Test.Method")
        .expect("descriptor should be retrievable by name");
    assert_eq!(descriptor.price, 10);
    assert_eq!(descriptor.required_call_flags, CallFlags::NONE);

    // Ensure hashing is stable
    let expected_hash = ScriptBuilder::hash_syscall("Test.Method").unwrap();
    assert_eq!(hash, expected_hash);
}

#[test]
fn invokes_registered_handler_via_instruction() {
    let mut engine = ExecutionEngine::new(None);
    let mut builder = ScriptBuilder::new();
    builder
        .emit_syscall("Test.Method")
        .expect("emit_syscall must succeed")
        .emit_opcode(OpCode::RET);
    let script = builder.to_script();
    engine.load_script(script, -1, 0).expect("load_script");

    let instruction = engine
        .current_context()
        .expect("context")
        .script()
        .get_instruction(0)
        .expect("instruction");

    let mut service = InteropService::new();
    service
        .register(VmInteropDescriptor {
            name: "Test.Method".to_string(),
            handler: Some(push_constant_handler),
            price: 0,
            required_call_flags: CallFlags::NONE,
        })
        .expect("registration");

    service
        .invoke_instruction(&mut engine, &instruction)
        .expect("invocation should succeed");

    let stack = engine
        .current_context()
        .expect("context")
        .evaluation_stack();
    assert_eq!(stack.len(), 1);
    assert_eq!(stack.peek(0).unwrap().as_int().unwrap(), 42.into());
}

#[derive(Default)]
struct RecordingHost {
    invoked: bool,
    last_hash: Option<u32>,
}

impl InteropHost for RecordingHost {
    fn invoke_syscall(&mut self, _engine: &mut ExecutionEngine, hash: u32) -> VmResult<()> {
        self.invoked = true;
        self.last_hash = Some(hash);
        Ok(())
    }
}

#[test]
fn delegates_to_host_when_handler_missing() {
    let mut engine = ExecutionEngine::new(None);
    let mut service = InteropService::new();
    let hash = service
        .register_host_descriptor("Host.Only", 0, CallFlags::NONE)
        .expect("host descriptor");

    let mut host = Box::new(RecordingHost::default());
    let host_ptr: *mut dyn InteropHost = &mut *host;
    engine.set_interop_host(host_ptr);

    service
        .invoke_by_hash(&mut engine, hash)
        .expect("host invocation");

    assert!(host.invoked);
    assert_eq!(host.last_hash, Some(hash));

    engine.clear_interop_host();
}

#[test]
fn registering_duplicate_descriptor_fails() {
    let mut service = InteropService::new();
    service
        .register(VmInteropDescriptor {
            name: "Test.Duplicate".to_string(),
            handler: Some(push_constant_handler),
            price: 0,
            required_call_flags: CallFlags::NONE,
        })
        .expect("first registration");

    let error = service
        .register(VmInteropDescriptor {
            name: "Test.Duplicate".to_string(),
            handler: Some(push_constant_handler),
            price: 0,
            required_call_flags: CallFlags::NONE,
        })
        .expect_err("duplicate should error");

    match error {
        VmError::InvalidOperation { .. } => {}
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn price_for_unknown_method_is_zero() {
    let service = InteropService::new();
    assert_eq!(service.get_price(b"Unknown"), 0);
}
