//! Host builds must free the ABI values lent to a host call that fails.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering},
};

use neo_vm::{
    CALLT_MARKER, OpCode, StackValue, SyscallProvider, VmState,
    interpret_with_stack_and_syscalls_at,
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

const PAYLOAD: [u8; 32] = [0x5a; 32];
/// `System.Runtime.Notify`, which declares two arguments.
const NOTIFY_API: u32 = 0x616f_0195;
const TOKEN: u16 = 7;

/// What the host does with the lent stack before it reports the failure.
#[derive(Clone, Copy, Debug)]
enum Lent {
    Untouched,
    Extended,
    Taken,
}

struct FailingHost {
    full_stack: bool,
    lent: Lent,
}

impl SyscallProvider for FailingHost {
    fn syscall(&mut self, api: u32, _ip: usize, stack: &mut Vec<StackValue>) -> Result<(), String> {
        match self.lent {
            Lent::Untouched => {}
            Lent::Extended => stack.extend(nested_initial_stack()),
            Lent::Taken => drop(std::mem::take(stack)),
        }
        Err(failure_message(api))
    }

    fn needs_full_stack(&self) -> bool {
        self.full_stack
    }
}

fn failure_message(api: u32) -> String {
    format!("host call 0x{api:08x} failed")
}

#[derive(Clone, Copy)]
enum HostCall {
    Syscall,
    Callt,
}

impl HostCall {
    fn emit(self, script: &mut Vec<u8>) {
        match self {
            Self::Syscall => {
                script.push(OpCode::SYSCALL.byte());
                script.extend(NOTIFY_API.to_le_bytes());
            }
            Self::Callt => {
                script.push(OpCode::CALLT.byte());
                script.extend(TOKEN.to_le_bytes());
            }
        }
    }

    /// The identifier the host sees: CALLT reaches it through the syscall
    /// channel with the token marker.
    fn api(self) -> u32 {
        match self {
            Self::Syscall => NOTIFY_API,
            Self::Callt => CALLT_MARKER | u32::from(TOKEN),
        }
    }
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

/// Pushes a payload on top of the initial stack and makes the host call.
fn host_call_body(call: HostCall) -> Vec<u8> {
    let mut body = vec![OpCode::PUSHDATA1.byte(), PAYLOAD.len() as u8];
    body.extend(PAYLOAD);
    call.emit(&mut body);
    body
}

/// Wraps the host call in a TRY whose catch block keeps the exception.
fn caught_script(call: HostCall) -> Vec<u8> {
    let body = host_call_body(call);
    // TRY and its two offsets, the body, the ENDTRY of the try block.
    let catch_offset = u8::try_from(3 + body.len() + 2).expect("short TRY offset");
    let mut script = vec![OpCode::TRY.byte(), catch_offset, 0];
    script.extend(body);
    script.extend([OpCode::ENDTRY.byte(), 4, OpCode::ENDTRY.byte(), 2]);
    script
}

/// Returns the bytes still live after `run`, measured once it is warmed up.
fn leaked_bytes(run: impl Fn()) -> usize {
    // Warm up the execution path before taking the allocation baseline.
    run();

    let before = LIVE_BYTES.load(Ordering::Relaxed);
    run();
    let after = LIVE_BYTES.load(Ordering::Relaxed);
    after.saturating_sub(before)
}

// A single test keeps the process-wide allocation counter free of noise from
// concurrently running tests.
#[test]
fn failed_host_calls_release_lent_abi_values() {
    let mut leaks = Vec::new();

    for (path, call, full_stack) in [
        ("syscall", HostCall::Syscall, false),
        ("full-stack syscall", HostCall::Syscall, true),
        ("CALLT", HostCall::Callt, false),
    ] {
        let message = failure_message(call.api());
        let uncaught = host_call_body(call);
        let caught = caught_script(call);
        let mut caught_stack = nested_initial_stack();
        caught_stack.push(StackValue::ByteString(PAYLOAD.to_vec()));
        caught_stack.push(StackValue::ByteString(message.clone().into_bytes()));

        for lent in [Lent::Untouched, Lent::Extended, Lent::Taken] {
            let leaked = leaked_bytes(|| {
                let mut host = FailingHost { full_stack, lent };
                let error = interpret_with_stack_and_syscalls_at(
                    &uncaught,
                    nested_initial_stack(),
                    0,
                    &mut host,
                )
                .expect_err("the host call fails");
                assert_eq!(error, message, "{path}, {lent:?}");
            });
            if leaked != 0 {
                leaks.push(format!(
                    "uncaught {path}, lent stack {lent:?}: {leaked} bytes"
                ));
            }

            let leaked = leaked_bytes(|| {
                let mut host = FailingHost { full_stack, lent };
                let result = interpret_with_stack_and_syscalls_at(
                    &caught,
                    nested_initial_stack(),
                    0,
                    &mut host,
                )
                .expect("the failure is caught");
                assert_eq!(result.state, VmState::Halt, "{path}, {lent:?}");
                assert_eq!(result.stack, caught_stack, "{path}, {lent:?}");
            });
            if leaked != 0 {
                leaks.push(format!(
                    "caught {path}, lent stack {lent:?}: {leaked} bytes"
                ));
            }
        }
    }

    assert!(
        leaks.is_empty(),
        "failed host calls leaked the values lent to the host: {leaks:#?}"
    );
}
