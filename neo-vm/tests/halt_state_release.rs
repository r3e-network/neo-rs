use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering},
};

use neo_vm::{
    ExecutionResult, OpCode, StackValue, SyscallProvider, VmState, interpret,
    interpret_with_stack_and_syscalls_at_with_initializer,
    interpret_with_stack_and_syscalls_at_with_result_limit,
};

struct LiveBytes;

static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for LiveBytes {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            LIVE_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        LIVE_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
    }
}

#[global_allocator]
static ALLOCATOR: LiveBytes = LiveBytes;

struct NoSyscalls;

impl SyscallProvider for NoSyscalls {
    fn syscall(
        &mut self,
        api: u32,
        _ip: usize,
        _stack: &mut Vec<StackValue>,
    ) -> Result<(), String> {
        Err(format!("unexpected syscall 0x{api:08x}"))
    }
}

const PAYLOAD: [u8; 32] = [0x5a; 32];

fn push_payload(script: &mut Vec<u8>) {
    script.extend([OpCode::PUSHDATA1.byte(), PAYLOAD.len() as u8]);
    script.extend(PAYLOAD);
}

/// Halts with a static field, an argument, a local, a mutated compound and two
/// evaluation stack items all still owning heap data.
fn populated_state_script() -> Vec<u8> {
    let mut script = vec![OpCode::INITSSLOT.byte(), 1];
    push_payload(&mut script);
    script.push(OpCode::STSFLD0.byte());
    push_payload(&mut script);
    script.extend([OpCode::INITSLOT.byte(), 1, 1]);
    push_payload(&mut script);
    script.extend([
        OpCode::PUSH1.byte(),
        OpCode::PACK.byte(),
        OpCode::STLOC0.byte(),
        OpCode::NEWARRAY0.byte(),
        OpCode::DUP.byte(),
    ]);
    push_payload(&mut script);
    script.push(OpCode::APPEND.byte());
    push_payload(&mut script);
    script.push(OpCode::RET.byte());
    script
}

fn nested_initial_stack() -> Vec<StackValue> {
    vec![StackValue::Array(vec![
        StackValue::ByteString(PAYLOAD.to_vec()),
        StackValue::Map(vec![(
            StackValue::Integer(1),
            StackValue::Buffer(PAYLOAD.to_vec()),
        )]),
    ])]
}

fn assert_halt_releases_state(
    scenario: &str,
    expected_stack: &[StackValue],
    run: impl Fn() -> ExecutionResult,
) {
    // Warm up the execution path before taking the allocation baseline.
    let warmup = run();
    assert_eq!(warmup.state, VmState::Halt, "{scenario}");
    assert_eq!(warmup.stack, expected_stack, "{scenario}");
    drop(warmup);

    let before = LIVE_BYTES.load(Ordering::Relaxed);
    let result = run();
    assert_eq!(result.state, VmState::Halt, "{scenario}");
    drop(result);
    let after = LIVE_BYTES.load(Ordering::Relaxed);

    assert_eq!(
        after,
        before,
        "{scenario}: halting leaked {leaked} bytes",
        leaked = after.saturating_sub(before)
    );
}

// A single test keeps the process-wide allocation counter free of noise from
// concurrently running tests.
#[test]
fn halted_executions_release_interpreter_state() {
    let script = populated_state_script();
    let payload = StackValue::ByteString(PAYLOAD.to_vec());
    let appended = StackValue::Array(vec![payload.clone()]);

    assert_halt_releases_state(
        "populated slots",
        &[appended.clone(), payload.clone()],
        || interpret(&script).expect("script halts"),
    );

    let initial = nested_initial_stack();
    let full_stack = [initial[0].clone(), appended, payload.clone()];
    for (limit, expected) in [
        (0, &full_stack[3..]),
        (1, &full_stack[2..]),
        (2, &full_stack[1..]),
        (8, &full_stack[..]),
    ] {
        assert_halt_releases_state(&format!("result limit {limit}"), expected, || {
            interpret_with_stack_and_syscalls_at_with_result_limit(
                &script,
                nested_initial_stack(),
                0,
                limit,
                &mut NoSyscalls,
            )
            .expect("script halts")
        });
    }

    let mut initializer = vec![OpCode::INITSSLOT.byte(), 1];
    push_payload(&mut initializer);
    initializer.extend([OpCode::STSFLD0.byte(), OpCode::RET.byte()]);
    let method_ip = initializer.len();
    initializer.extend([OpCode::LDSFLD0.byte(), OpCode::RET.byte()]);
    assert_halt_releases_state("static initializer", &[initial[0].clone(), payload], || {
        interpret_with_stack_and_syscalls_at_with_initializer(
            &initializer,
            nested_initial_stack(),
            method_ip,
            0,
            &mut NoSyscalls,
        )
        .expect("script halts")
    });
}
