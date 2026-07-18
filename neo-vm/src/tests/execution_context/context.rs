#![allow(dead_code)]

use super::*;
use crate::OpCode;
use crate::stack_item::StackItem;
use num_bigint::BigInt;

#[derive(Default)]
struct TestFlag {
    flag: bool,
}

#[derive(Default)]
struct TestState {
    flag: bool,
    values: Vec<i32>,
}

#[test]
fn test_execution_context_creation() {
    let script_bytes = vec![
        OpCode::PUSH1.byte(),
        OpCode::PUSH2.byte(),
        OpCode::ADD.byte(),
    ];
    let script = Script::new_relaxed(script_bytes);
    let reference_counter = ReferenceCounter::new();

    let context = ExecutionContext::<()>::new(script, -1, &reference_counter);

    assert_eq!(context.instruction_pointer(), 0);
    assert_eq!(context.rvcount(), -1);
    assert_eq!(
        context
            .current_instruction()
            .expect("intermediate value should exist")
            .opcode(),
        OpCode::PUSH1
    );
    assert!(context.evaluation_stack().is_empty());
    assert!(context.try_stack().is_none());
}

#[test]
fn test_move_next() {
    let script_bytes = vec![
        OpCode::PUSH1.byte(),
        OpCode::PUSH2.byte(),
        OpCode::ADD.byte(),
    ];
    let script = Script::new_relaxed(script_bytes);
    let reference_counter = ReferenceCounter::new();

    let mut context = ExecutionContext::<()>::new(script, -1, &reference_counter);

    assert_eq!(
        context
            .current_instruction()
            .expect("intermediate value should exist")
            .opcode(),
        OpCode::PUSH1
    );

    context.move_next().expect("VM operation should succeed");
    assert_eq!(context.instruction_pointer(), 1);
    assert_eq!(
        context
            .current_instruction()
            .expect("intermediate value should exist")
            .opcode(),
        OpCode::PUSH2
    );

    context.move_next().expect("VM operation should succeed");
    assert_eq!(context.instruction_pointer(), 2);
    assert_eq!(
        context
            .current_instruction()
            .expect("intermediate value should exist")
            .opcode(),
        OpCode::ADD
    );

    context.move_next().expect("VM operation should succeed");
    assert_eq!(context.instruction_pointer(), 3);
    assert!(context.current_instruction().is_err());
}

#[test]
fn test_try_stack() {
    let script_bytes = vec![OpCode::NOP.byte()];
    let script = Script::new_relaxed(script_bytes);
    let reference_counter = ReferenceCounter::new();

    let mut context = ExecutionContext::<()>::new(script, -1, &reference_counter);

    // Initially, try_stack is None
    assert!(context.try_stack().is_none());

    // Create a try stack with one context
    use crate::ExceptionHandlingContext;
    use crate::ExceptionHandlingState;
    let mut try_stack = Vec::new();
    let try_context = ExceptionHandlingContext::new(10, 20);
    try_stack.push(try_context);

    // Set the try stack
    context.set_try_stack(Some(try_stack));

    // Check that the try stack is set
    assert!(context.try_stack().is_some());
    assert_eq!(
        context
            .try_stack()
            .expect("intermediate value should exist")
            .len(),
        1
    );
    assert_eq!(
        context.try_stack().expect("VM operation should succeed")[0].catch_pointer(),
        10
    );
    assert_eq!(
        context.try_stack().expect("Operation failed")[0].finally_pointer(),
        20
    );

    // Modify the try stack
    if let Some(stack) = context.try_stack_mut() {
        let exception_context = &mut stack[0];
        exception_context.set_state(ExceptionHandlingState::Catch);
    }

    // Check that the modification was applied
    assert_eq!(
        context.try_stack().expect("Operation failed")[0].state(),
        ExceptionHandlingState::Catch
    );
}

#[test]
fn test_slot() {
    let reference_counter = ReferenceCounter::new();

    let items = vec![
        StackItem::from_int(1),
        StackItem::from_int(2),
        StackItem::from_int(3),
    ];

    let mut slot = Slot::with_items(items, reference_counter.clone());

    assert_eq!(slot.len(), 3);
    assert_eq!(
        slot.get(0)
            .expect("Index out of bounds")
            .as_int()
            .expect("VM operation should succeed"),
        BigInt::from(1)
    );
    assert_eq!(
        slot.get(1)
            .expect("Index out of bounds")
            .as_int()
            .expect("VM operation should succeed"),
        BigInt::from(2)
    );
    assert_eq!(
        slot.get(2)
            .expect("Index out of bounds")
            .as_int()
            .expect("VM operation should succeed"),
        BigInt::from(3)
    );

    slot.set(1, StackItem::from_int(42)).unwrap();
    assert_eq!(
        slot.get(1)
            .expect("Index out of bounds")
            .as_int()
            .expect("VM operation should succeed"),
        BigInt::from(42)
    );

    assert!(slot.set(5, StackItem::from_int(5)).is_err());

    slot.clear_references();
    assert_eq!(slot.len(), 3);
    assert!(slot.iter().all(|item| item.is_null()));
}

#[test]
fn test_next_instruction() {
    let script_bytes = vec![
        OpCode::PUSH1.byte(),
        OpCode::PUSH2.byte(),
        OpCode::ADD.byte(),
        OpCode::RET.byte(),
    ];

    let script = Script::new_relaxed(script_bytes);
    let reference_counter = ReferenceCounter::new();

    let context = ExecutionContext::<()>::new(script, -1, &reference_counter);

    // Test current instruction
    let current = context
        .current_instruction()
        .expect("VM operation should succeed");
    assert_eq!(current.opcode(), OpCode::PUSH1);

    // Test next instruction
    let next = context
        .next_instruction()
        .expect("VM operation should succeed");
    assert_eq!(next.opcode(), OpCode::PUSH2);
}

#[test]
fn test_clone() {
    let script_bytes = vec![
        OpCode::PUSH1.byte(),
        OpCode::PUSH2.byte(),
        OpCode::ADD.byte(),
        OpCode::RET.byte(),
    ];

    let script = Script::new_relaxed(script_bytes);
    let reference_counter = ReferenceCounter::new();

    let mut context = ExecutionContext::<()>::new(script, -1, &reference_counter);

    // Push a value onto the stack
    context
        .push(StackItem::from_int(42))
        .expect("push should succeed");

    // Clone the context
    let clone = context.clone();

    // Clone() shares script/evaluation stack/static fields (C# semantics).
    assert_eq!(clone.script().to_array(), context.script().to_array());
    assert_eq!(clone.evaluation_stack().len(), 1);
    assert!(clone.shares_evaluation_stack_with(&context));

    // Check that the clone has the same instruction pointer
    assert_eq!(clone.instruction_pointer(), context.instruction_pointer());

    // C# Clone() sets rvcount = 0 for CALL.
    assert_eq!(clone.rvcount(), 0);

    // Clone with a different position
    let clone_with_position = context
        .clone_with_position(2)
        .expect("clone position is valid");

    // Check that the clone has a different instruction pointer
    assert_eq!(clone_with_position.instruction_pointer(), 2);
}

#[test]
fn test_typed_state_is_shared_across_clones() {
    let script_bytes = vec![OpCode::NOP.byte()];
    let script = Script::new_relaxed(script_bytes);
    let reference_counter = ReferenceCounter::new();

    let context = ExecutionContext::<TestState>::new(script, -1, &reference_counter);

    let flag_state = context.state();
    {
        let mut state = flag_state.lock();
        assert!(!state.flag);
        state.flag = true;
        state.values.push(100);
    }

    let clone = context.clone();
    assert!(context.shares_state_with(&clone));
    let cloned_stack_state = clone.state();
    {
        let mut state = cloned_stack_state.lock();
        assert!(state.flag);
        assert_eq!(state.values.pop(), Some(100));
        state.values.push(200);
    }

    let original_stack_state = context.state();
    {
        let mut state = original_stack_state.lock();
        assert!(state.flag);
        assert_eq!(state.values.pop(), Some(200));
    }
}

#[test]
fn test_typed_state_replacement_and_reference_counter_clone_reset() {
    let script_bytes = vec![OpCode::NOP.byte()];
    let script = Script::new_relaxed(script_bytes);
    let reference_counter = ReferenceCounter::new();

    let context = ExecutionContext::new_with_state(
        script,
        -1,
        &reference_counter,
        TestState {
            flag: true,
            values: vec![100],
        },
    );

    let clone = context.clone();
    let previous = context.replace_state(TestState {
        flag: false,
        values: vec![1, 2, 3],
    });
    assert!(previous.flag);
    assert_eq!(previous.values, vec![100]);

    let shared_state = clone.state();
    {
        let state = shared_state.lock();
        assert!(!state.flag);
        assert_eq!(state.values, vec![1, 2, 3]);
    }

    let fresh_counter = ReferenceCounter::new();
    let fresh_clone = context
        .clone_for_reference_counter(&fresh_counter)
        .expect("clone with a fresh reference counter");
    assert!(!fresh_clone.shares_state_with(&context));

    let fresh_state = fresh_clone.state();
    let fresh_state = fresh_state.lock();
    assert!(!fresh_state.flag);
    assert!(fresh_state.values.is_empty());
}
