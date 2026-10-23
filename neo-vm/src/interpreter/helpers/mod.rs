pub(crate) use super::runtime_types::{
    CompoundIds, StackValue, into_abi_value, structurally_equal, to_abi_stack, to_abi_value,
};
use crate::{
    STACK_VALUE_CODEC_TAG_ARRAY, STACK_VALUE_CODEC_TAG_BIG_INTEGER, STACK_VALUE_CODEC_TAG_BOOLEAN,
    STACK_VALUE_CODEC_TAG_BUFFER, STACK_VALUE_CODEC_TAG_BYTESTRING, STACK_VALUE_CODEC_TAG_INTEGER,
    STACK_VALUE_CODEC_TAG_INTEROP, STACK_VALUE_CODEC_TAG_ITERATOR, STACK_VALUE_CODEC_TAG_MAP,
    STACK_VALUE_CODEC_TAG_NULL, STACK_VALUE_CODEC_TAG_POINTER, STACK_VALUE_CODEC_TAG_STRUCT,
};
use core::cell::UnsafeCell;

const POST_SYSCALL_STACK_HEADROOM: usize = 8;

// The retained-state buffers only exist for the riscv32 build. Every writer is
// inside a `cfg!(target_arch = "riscv32")` branch, so on every other target the
// code compiles but never touches the storage. Sizing them to zero there keeps
// the five statics below out of BSS (they would reserve 10 MiB) and leaves no
// interior-mutable bytes that a stray call could alias.
#[cfg(target_arch = "riscv32")]
const RETAINED_PREFIX_BUF_SIZE: usize = 2 * 1024 * 1024;
#[cfg(not(target_arch = "riscv32"))]
const RETAINED_PREFIX_BUF_SIZE: usize = 0;

const MAX_RETAINED_DECODE_DEPTH: usize = 64;
const MAX_RETAINED_COLLECTION_LEN: usize = 4096;

pub(crate) struct RetainedPrefixBuffer(UnsafeCell<[u8; RETAINED_PREFIX_BUF_SIZE]>);

// SAFETY: on riscv32 the VM executes on a single thread, and the remaining
// targets never reach the buffers (zero-sized, and every accessor is behind a
// `cfg!(target_arch = "riscv32")` check). Any future multi-threaded use of the
// riscv32 path would have to replace this with per-engine storage instead.
unsafe impl Sync for RetainedPrefixBuffer {}

impl RetainedPrefixBuffer {
    const fn new() -> Self {
        Self(UnsafeCell::new([0; RETAINED_PREFIX_BUF_SIZE]))
    }

    #[allow(clippy::mut_from_ref)]
    pub(crate) unsafe fn as_mut_slice(&self) -> &mut [u8] {
        unsafe { &mut *self.0.get() }
    }

    pub(crate) unsafe fn as_slice(&self, len: usize) -> &[u8] {
        unsafe { &(&*self.0.get())[..len] }
    }
}

static RETAINED_STACK_BUF: RetainedPrefixBuffer = RetainedPrefixBuffer::new();
pub(crate) static RETAINED_ARGS_BUF: RetainedPrefixBuffer = RetainedPrefixBuffer::new();
static RETAINED_LOCALS_BUF: RetainedPrefixBuffer = RetainedPrefixBuffer::new();
static RETAINED_STATIC_FIELDS_BUF: RetainedPrefixBuffer = RetainedPrefixBuffer::new();
static RETAINED_CONSUMED_MUTATIONS_BUF: RetainedPrefixBuffer = RetainedPrefixBuffer::new();
#[cfg(target_arch = "riscv32")]
static RETAINED_INITIAL_STACK_BUF: RetainedPrefixBuffer = RetainedPrefixBuffer::new();
#[cfg(target_arch = "riscv32")]
pub(crate) static RETAINED_CALL_STACK_BUF: RetainedPrefixBuffer = RetainedPrefixBuffer::new();

use super::SyscallProvider;
use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};

mod bridge;
mod retained;
mod values;

pub(crate) use bridge::*;
pub(crate) use retained::*;
pub(crate) use values::*;
