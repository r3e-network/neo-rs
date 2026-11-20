//! Evaluation-stack backing-buffer reuse.
//!
//! These tests cover the two things the allocator must get right:
//!
//! * the reuse itself - a returning script hands its backing allocation back
//!   and the next context starts on it instead of allocating; and
//! * semantic parity - recycling must not change states, results, reference
//!   counts, `CALL` stack sharing, context unload or fault unwinding, and it
//!   must never retain an oversized buffer.

use super::*;
use crate::OpCode;
use crate::StackItemType;
use crate::memory::{StackAllocator, StackAllocatorLimits, StackBufferRejection};

fn simple_script() -> Script {
    Script::new_relaxed(vec![
        OpCode::PUSH1.byte(),
        OpCode::PUSH2.byte(),
        OpCode::ADD.byte(),
        OpCode::RET.byte(),
    ])
}

/// `PUSH1; CALL +3; RET; PUSH2; RET` - the callee shares the caller's stack.
fn call_script() -> Script {
    Script::new_relaxed(vec![
        OpCode::PUSH1.byte(),
        OpCode::CALL.byte(),
        3, // jump from the CALL instruction to the `PUSH2` at offset 4
        OpCode::RET.byte(),
        OpCode::PUSH2.byte(),
        OpCode::RET.byte(),
    ])
}

/// The bounds that disable the pool entirely: the pre-reuse behaviour.
const DISABLED: StackAllocatorLimits = StackAllocatorLimits::new(0, 0, 0);

/// Everything about one execution that must not depend on the pool.
///
/// The counters are carried along separately as the reuse evidence; only
/// `state`, `references` and `result` are compared for parity.
#[derive(Debug)]
struct Outcome {
    state: VMState,
    references: isize,
    result: Vec<String>,
    acquired: u64,
    recycled: u64,
    retained_buffers: usize,
    retained_bytes: usize,
    rejected: u64,
}

/// Structural rendering of a stack item.
///
/// `Debug` embeds a process-wide compound id, which is different for two runs
/// of the same script, so parity compares this instead.
fn describe(item: &StackItem) -> String {
    let kind = format!("{:?}", item.stack_item_type());
    if let Ok(items) = item.as_array() {
        let inner: Vec<String> = items.iter().map(describe).collect();
        return format!("{kind}[{}]", inner.join(","));
    }
    match item.stack_item_type() {
        StackItemType::Integer => format!("{kind}({})", item.as_int().expect("integer")),
        _ => kind,
    }
}

fn run(limits: StackAllocatorLimits, script: Script) -> Outcome {
    let mut engine = ExecutionEngine::new(None);
    engine.set_stack_allocator_limits(limits);
    engine.load_script(script, -1, 0).expect("load script");
    let state = engine.execute();
    let references = engine.reference_counter().signed_count();
    let result = engine.result_stack().iter().map(describe).collect();
    let stats = engine.stack_allocator_stats();
    Outcome {
        state,
        references,
        result,
        acquired: stats.acquired,
        recycled: stats.recycled,
        retained_buffers: stats.retained_buffers,
        retained_bytes: stats.retained_bytes,
        rejected: stats.rejected(),
    }
}

/// Runs the same script with the pool enabled and disabled and asserts that
/// every observable except the allocator counters is identical.
fn assert_parity(build: impl Fn() -> Script) -> Outcome {
    let pooled = run(StackAllocatorLimits::default(), build());
    let unpooled = run(DISABLED, build());
    assert_eq!(
        pooled.state, unpooled.state,
        "recycling changed the final state"
    );
    assert_eq!(
        pooled.references, unpooled.references,
        "recycling changed the reference count"
    );
    assert_eq!(
        pooled.result, unpooled.result,
        "recycling changed the result stack"
    );
    assert_eq!(unpooled.recycled, 0, "a disabled pool must not recycle");
    assert_eq!(unpooled.retained_buffers, 0);
    assert!(
        unpooled.rejected >= 1,
        "the disabled pool must reject the returned allocation"
    );
    assert_eq!(
        pooled.acquired, 0,
        "a single load has nothing to acquire from"
    );
    if pooled.retained_buffers == 0 {
        assert_eq!(pooled.retained_bytes, 0);
    } else {
        assert!(
            pooled.retained_bytes >= pooled.retained_buffers * std::mem::size_of::<StackItem>(),
            "retained bytes are smaller than the buffers they describe"
        );
    }
    pooled
}

#[test]
fn a_returning_script_returns_its_allocation_to_the_pool() {
    let mut engine = ExecutionEngine::new(None);
    engine
        .load_script(simple_script(), -1, 0)
        .expect("load script");
    assert_eq!(engine.stack_allocator_stats().retained_buffers, 0);

    assert_eq!(engine.execute(), VMState::HALT);

    let stats = engine.stack_allocator_stats();
    assert_eq!(
        stats.recycled, 1,
        "RET hands the dead context's buffer back"
    );
    assert_eq!(stats.retained_buffers, 1);
    assert_eq!(stats.retained_capacity, 32);
    assert_eq!(stats.retained_bytes, 32 * std::mem::size_of::<StackItem>());
    assert_eq!(stats.acquired, 0, "the entry context allocated normally");
}

#[test]
fn the_next_load_reuses_the_recycled_allocation() {
    let mut engine = ExecutionEngine::new(None);
    engine
        .load_script(simple_script(), -1, 0)
        .expect("load script");
    assert_eq!(engine.execute(), VMState::HALT);

    engine
        .load_script(simple_script(), -1, 0)
        .expect("load script");

    let stats = engine.stack_allocator_stats();
    assert_eq!(stats.acquired, 1, "second context started on the pool");
    assert_eq!(
        stats.retained_buffers, 0,
        "the buffer was taken, not copied"
    );
    assert_eq!(stats.retained_bytes, 0);
    let context = engine.current_context().expect("context");
    assert_eq!(
        context.evaluation_stack().capacity(),
        32,
        "reused allocation keeps its capacity and needs no growth"
    );
}

#[test]
fn repeated_loads_keep_the_retained_pool_small() {
    let mut engine = ExecutionEngine::new(None);
    let limit = engine.stack_allocator_limits().max_retained_buffers;

    for round in 0..16 {
        // `execute` only runs while the state is not terminal, so a completed
        // run has to be reset before the next load. This mirrors the
        // pre-existing engine behaviour and keeps the rounds independent.
        engine.set_state(VMState::NONE);
        engine
            .load_script(simple_script(), -1, 0)
            .expect("load script");
        assert_eq!(engine.execute(), VMState::HALT);
        let stats = engine.stack_allocator_stats();
        assert!(
            stats.retained_buffers <= limit,
            "round {round} grew the pool past its bound"
        );
        assert!(stats.retained_bytes <= engine.stack_allocator_limits().max_retained_bytes);
    }

    let stats = engine.stack_allocator_stats();
    // One allocation for the first load, one recycle per load, and every later
    // load starts on the recycled buffer.
    assert_eq!(stats.recycled, 16);
    assert_eq!(stats.acquired, 15);
    assert_eq!(stats.retained_buffers, 1);
}

#[test]
fn an_oversized_evaluation_stack_is_not_retained() {
    let mut engine = ExecutionEngine::new(None);
    engine.set_stack_allocator_limits(StackAllocatorLimits::new(8, 64, 64 * 1024));

    // 200 pushes grow the backing buffer past 64 elements.
    let script = Script::new_relaxed(vec![OpCode::PUSH1.byte(); 200]);
    engine.load_script(script, -1, 0).expect("load script");
    assert_eq!(engine.execute(), VMState::HALT);

    let stats = engine.stack_allocator_stats();
    assert_eq!(stats.rejected_oversized, 1);
    assert_eq!(stats.retained_buffers, 0);
    assert_eq!(stats.retained_bytes, 0);
}

#[test]
fn disabling_the_pool_rejects_every_return() {
    let mut engine = ExecutionEngine::new(None);
    engine.set_stack_allocator_limits(StackAllocatorLimits::new(0, 0, 0));
    engine
        .load_script(simple_script(), -1, 0)
        .expect("load script");
    assert_eq!(engine.execute(), VMState::HALT);

    let stats = engine.stack_allocator_stats();
    assert_eq!(stats.recycled, 0);
    assert_eq!(stats.retained_buffers, 0);
    assert!(stats.rejected() >= 1);
}

#[test]
fn a_shared_context_never_hands_its_stack_back() {
    let reference_counter = ReferenceCounter::new();
    let mut caller = ExecutionContext::new(simple_script(), -1, &reference_counter);
    let mut callee = caller.clone_with_position(0);
    assert!(caller.shares_evaluation_stack_with(&callee));

    // Both handles alias the same evaluation stack, so neither may detach it.
    assert!(callee.take_exclusive_evaluation_buffer().is_none());
    assert!(caller.take_exclusive_evaluation_buffer().is_none());

    // Once the callee is gone the caller is the sole owner.
    drop(callee);
    let buffer = caller
        .take_exclusive_evaluation_buffer()
        .expect("sole owner detaches its buffer");
    assert!(buffer.is_empty());
    assert!(buffer.capacity() >= 32);

    // Detaching twice leaves the empty `Vec` that `mem::take` put in place.
    // The allocator refuses an allocation-less buffer, so it is dropped.
    let empty = caller
        .take_exclusive_evaluation_buffer()
        .expect("the stack still owns the empty vector left behind");
    assert!(empty.is_empty());
    assert_eq!(empty.capacity(), 0);
    assert_eq!(
        StackAllocator::default().recycle(empty),
        Err(StackBufferRejection::Empty)
    );
}

#[test]
fn call_sharing_keeps_working_when_recycling_is_enabled() {
    let mut engine = ExecutionEngine::new(None);
    engine
        .load_script(call_script(), -1, 0)
        .expect("load script");
    assert_eq!(engine.execute(), VMState::HALT);

    // The callee shares the caller's stack, so copied values still flow through
    // the shared stack: both pushes end up on the result stack.
    let results: Vec<i64> = engine
        .result_stack()
        .iter()
        .map(|item| {
            let value = item.as_int().expect("integer");
            i64::try_from(value).expect("small integer")
        })
        .collect();
    assert_eq!(results, vec![1, 2]);

    // Exactly one allocation was reclaimed: the outermost context. The shared
    // callee context was never detached while its alias was alive.
    let stats = engine.stack_allocator_stats();
    assert_eq!(stats.recycled, 1);
    assert_eq!(stats.retained_buffers, 1);
}

#[test]
fn remove_context_keeps_the_stack_that_ret_recycles() {
    let mut engine = ExecutionEngine::new(None);
    engine
        .load_script(simple_script(), -1, 0)
        .expect("load script");
    engine.push(StackItem::from_int(1)).expect("push");
    engine.push(StackItem::from_int(2)).expect("push");

    let context = engine.remove_context(0).expect("remove context");
    // A removed context escapes to the caller, so its stack stays intact and
    // nothing is recycled.
    assert_eq!(context.evaluation_stack().len(), 2);
    assert_eq!(engine.stack_allocator_stats().recycled, 0);
    assert_eq!(engine.stack_allocator_stats().retained_buffers, 0);
}

#[test]
fn fault_unwinding_recycles_the_abandoned_stack() {
    // The array is still on the evaluation stack when THROW unwinds, so
    // clearing the recycled buffer is what releases it. That release must go
    // through `Drop`, not through the reference counter.
    let build = || {
        Script::new_relaxed(vec![
            OpCode::PUSH1.byte(),
            OpCode::NEWARRAY.byte(),
            OpCode::PUSH1.byte(),
            OpCode::THROW.byte(),
        ])
    };
    let pooled = assert_parity(build);
    assert_eq!(pooled.state, VMState::FAULT);
    assert!(
        pooled.references > 0,
        "the abandoned array still owns its references"
    );
    assert_eq!(
        pooled.recycled, 1,
        "THROW hands the dead context's buffer back"
    );
    assert_eq!(pooled.retained_buffers, 1);
}

#[test]
fn running_off_the_end_of_a_script_recycles_too() {
    // No RET: the engine substitutes one when the instruction pointer runs off
    // the end of the script. That path must recycle like an explicit RET.
    let build = || Script::new_relaxed(vec![OpCode::PUSH1.byte()]);
    let pooled = assert_parity(build);
    assert_eq!(pooled.state, VMState::HALT);
    assert_eq!(pooled.result.len(), 1);
    assert_eq!(pooled.recycled, 1);
    assert_eq!(pooled.retained_buffers, 1);
}

#[test]
fn recycling_preserves_state_references_and_results() {
    // The disabled pool is the pre-reuse behaviour: every dead context's buffer
    // is dropped straight away. Both runs must be indistinguishable.
    for build in [simple_script, call_script] {
        let pooled = assert_parity(build);
        assert_eq!(pooled.state, VMState::HALT);
        assert!(pooled.recycled >= 1, "the pooled run did not recycle");
    }
}

#[test]
fn a_compound_stack_item_survives_recycling_of_its_stack() {
    // Build a script whose result is a compound item so the reference counter
    // takes part in the comparison.
    let build = || {
        Script::new_relaxed(vec![
            OpCode::PUSH1.byte(),
            OpCode::NEWARRAY.byte(),
            OpCode::RET.byte(),
        ])
    };
    let pooled = assert_parity(build);
    assert_eq!(pooled.state, VMState::HALT);
    assert_eq!(pooled.result.len(), 1, "the array reached the result stack");
    assert!(
        pooled.references > 0,
        "the returned array keeps its references after the buffer is recycled"
    );
}
