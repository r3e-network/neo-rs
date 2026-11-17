use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering},
};

use neo_vm::{OpCode, VmState, interpret};

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

#[test]
fn active_call_frames_release_owned_values_on_fault() {
    let mut script = vec![OpCode::PUSHDATA1.byte(), 32];
    script.extend([0x5a; 32]);
    script.extend([
        OpCode::INITSLOT.byte(),
        1,
        0,
        OpCode::STLOC0.byte(),
        OpCode::CALL.byte(),
        3,
        OpCode::RET.byte(),
        OpCode::ABORT.byte(),
    ]);

    // Warm up the execution path before taking the allocation baseline.
    let warmup = interpret(&script).expect("ABORT returns a fault result");
    assert_eq!(warmup.state, VmState::Fault);
    drop(warmup);

    let before = LIVE_BYTES.load(Ordering::Relaxed);
    let result = interpret(&script).expect("ABORT returns a fault result");
    assert_eq!(result.state, VmState::Fault);
    drop(result);
    let after = LIVE_BYTES.load(Ordering::Relaxed);

    assert_eq!(
        after,
        before,
        "faulting with an active call frame leaked {leaked} bytes",
        leaked = after.saturating_sub(before)
    );
}
