//!
//! Tests for ExecutionContext state management and functionality.

use neo_vm::{
    exception_handling_context::ExceptionHandlingContext,
    exception_handling_state::ExceptionHandlingState, execution_context::ExecutionContext,
    op_code::OpCode, reference_counter::ReferenceCounter, script::Script, slot::Slot,
};
use std::sync::Mutex;

#[derive(Clone, Default)]
struct TestState {
    flag: bool,
}

impl TestState {
    fn new(flag: bool) -> Self {
        Self { flag }
    }

    fn flag(&self) -> bool {
        self.flag
    }

    fn set_flag(&mut self, value: bool) {
        self.flag = value;
    }
}

#[derive(Default)]
struct CounterState {
    value: i32,
}

impl CounterState {
    fn value(&self) -> i32 {
        self.value
    }
}

#[test]
fn test_execution_context_state() {
    let script = Script::new(vec![], false).unwrap();
    let reference_counter = ReferenceCounter::new();
    let context = ExecutionContext::new(script, -1, &reference_counter);

    let state_handle = context.get_state_with_factory::<TestState, _>(|| TestState::new(true));
    {
        let mut state = state_handle.lock().expect("state mutex poisoned");
        assert!(state.flag());
        state.set_flag(false);
    }

    let state_again = context.get_state::<TestState>();
    {
        let state = state_again.lock().expect("state mutex poisoned");
        assert!(!state.flag());
    }

    let clone = context.clone();
    {
        let cloned_state = clone.get_state::<TestState>();
        let mut state = cloned_state.lock().expect("clone state mutex poisoned");
        state.set_flag(true);
    }

    let shared_state = context.get_state::<TestState>();
    {
        let state = shared_state.lock().expect("state mutex poisoned");
        assert!(state.flag());
    }

    let counter = context.get_state::<CounterState>();
    {
        let counter = counter.lock().expect("counter mutex poisoned");
        assert_eq!(counter.value(), 0);
    }
}

#[test]
fn test_execution_context_instruction_pointer() {
    let script_bytes = vec![OpCode::PUSH1 as u8, OpCode::PUSH2 as u8, OpCode::ADD as u8];
    let script = Script::new(script_bytes, false).unwrap();
    let reference_counter = ReferenceCounter::new();
    let mut context = ExecutionContext::new(script, 0, &reference_counter);

    assert_eq!(context.instruction_pointer(), 0);
    assert_eq!(
        context
            .current_instruction()
            .expect("current instruction must exist")
            .opcode(),
        OpCode::PUSH1
    );

    context.move_next().expect("move_next should succeed");
    assert_eq!(context.instruction_pointer(), 1);
    assert_eq!(
        context
            .current_instruction()
            .expect("current instruction must exist")
            .opcode(),
        OpCode::PUSH2
    );

    context.move_next().expect("move_next should succeed");
    assert_eq!(context.instruction_pointer(), 2);
    assert_eq!(
        context
            .current_instruction()
            .expect("current instruction must exist")
            .opcode(),
        OpCode::ADD
    );

    context.move_next().expect("move_next should succeed");
    assert!(context.current_instruction().is_err());
}

#[test]
fn test_execution_context_evaluation_stack() {
    let script = Script::new(vec![], false).unwrap();
    let reference_counter = ReferenceCounter::new();
    let context = ExecutionContext::new(script, -1, &reference_counter);

    assert_eq!(context.evaluation_stack().len(), 0);
}

#[test]
fn test_execution_context_local_variables() {
    let script = Script::new(vec![], false).unwrap();
    let reference_counter = ReferenceCounter::new();
    let mut context = ExecutionContext::new(script, -1, &reference_counter);

    context
        .init_slot(3, 0)
        .expect("slot initialisation must succeed");

    let locals = context
        .local_variables()
        .expect("locals should be initialised");
    assert_eq!(locals.count(), 3);
}

#[test]
fn test_execution_context_arguments() {
    let script = Script::new(vec![], false).unwrap();
    let reference_counter = ReferenceCounter::new();
    let mut context = ExecutionContext::new(script, -1, &reference_counter);

    context
        .init_slot(0, 2)
        .expect("slot initialisation must succeed");

    let arguments = context
        .arguments()
        .expect("arguments should be initialised");
    assert_eq!(arguments.count(), 2);
}

#[test]
fn test_execution_context_static_fields() {
    let script = Script::new(vec![], false).unwrap();
    let reference_counter = ReferenceCounter::new();
    let mut context = ExecutionContext::new(script, -1, &reference_counter);

    let static_slot = Slot::new(4, reference_counter.clone());
    context.set_static_fields(Some(static_slot));

    let statics = context
        .static_fields()
        .expect("static fields should be set");
    assert_eq!(statics.count(), 4);
}

#[test]
fn test_execution_context_try_stack() {
    let script = Script::new(vec![], false).unwrap();
    let reference_counter = ReferenceCounter::new();
    let mut context = ExecutionContext::new(script, -1, &reference_counter);

    assert!(!context.has_try_context());
    assert_eq!(context.try_stack_len(), 0);

    context.push_try_context(ExceptionHandlingContext::new(10, 20));

    assert!(context.has_try_context());
    assert_eq!(context.try_stack_len(), 1);
    assert_eq!(
        context
            .try_stack_last()
            .expect("try context must exist")
            .catch_pointer(),
        10
    );

    if let Some(current) = context.try_stack_last_mut() {
        current.set_state(ExceptionHandlingState::Catch);
    }

    assert_eq!(
        context
            .try_stack_last()
            .expect("try context must exist")
            .state(),
        ExceptionHandlingState::Catch
    );

    assert_eq!(
        context
            .pop_try_context()
            .expect("try context must exist")
            .state(),
        ExceptionHandlingState::Catch
    );
    assert!(!context.has_try_context());
}

#[test]
fn test_execution_context_script_hash() {
    let script_bytes = vec![OpCode::PUSH1 as u8, OpCode::PUSH2 as u8, OpCode::ADD as u8];
    let script = Script::new(script_bytes, false).unwrap();
    let reference_counter = ReferenceCounter::new();
    let context = ExecutionContext::new(script, -1, &reference_counter);

    let hash = context.script_hash();
    assert!(hash.iter().any(|byte| *byte != 0));
}
