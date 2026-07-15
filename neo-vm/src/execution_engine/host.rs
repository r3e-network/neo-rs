//! Unsafe host-pointer bridge for VM interop callbacks.
//!
//! The execution engine keeps host callbacks allocation-free on the VM hot path
//! by storing a thin host pointer plus monomorphized callback thunks for the
//! concrete host type. This module confines all unsafe pointer dereferences and
//! documents the invariants required by the parent `ExecutionEngine` facade.

use crate::Instruction;
use crate::error::VmResult;
use crate::execution_context::ExecutionContext;
use crate::interop_service::InteropHost;
use std::ptr::NonNull;

use super::ExecutionEngine;

type ContextCallback<S> =
    unsafe fn(NonNull<()>, &mut ExecutionEngine<S>, &ExecutionContext<S>) -> VmResult<()>;
type InstructionCallback<S> =
    unsafe fn(NonNull<()>, &mut ExecutionEngine<S>, &Instruction) -> VmResult<()>;
type SyscallCallback<S> = unsafe fn(NonNull<()>, &mut ExecutionEngine<S>, u32) -> VmResult<()>;
type CalltCallback<S> = unsafe fn(NonNull<()>, &mut ExecutionEngine<S>, u16) -> VmResult<()>;

/// Static callback table for one concrete host type.
struct HostCallbacks<S = ()> {
    on_context_loaded: ContextCallback<S>,
    on_context_unloaded: ContextCallback<S>,
    pre_execute_instruction: InstructionCallback<S>,
    post_execute_instruction: InstructionCallback<S>,
    invoke_syscall: SyscallCallback<S>,
    on_callt: CalltCallback<S>,
}

impl<S> HostCallbacks<S> {
    const fn for_host<H: InteropHost<S>>() -> Self {
        Self {
            on_context_loaded: on_context_loaded::<H, S>,
            on_context_unloaded: on_context_unloaded::<H, S>,
            pre_execute_instruction: pre_execute_instruction::<H, S>,
            post_execute_instruction: post_execute_instruction::<H, S>,
            invoke_syscall: invoke_syscall::<H, S>,
            on_callt: on_callt::<H, S>,
        }
    }
}

impl<S> Copy for HostCallbacks<S> {}

impl<S> Clone for HostCallbacks<S> {
    fn clone(&self) -> Self {
        *self
    }
}

/// A wrapper around a raw host pointer that centralizes all unsafe access.
///
/// # Safety
///
/// This type encapsulates the raw host pointer that the execution engine uses to
/// call back into the host environment (e.g. `ApplicationEngine`). The pointer is
/// thin (`*mut ()` internally); callback function pointers are monomorphized for
/// the concrete host type when the host is attached.
///
/// The following invariants **must** be upheld by the caller who creates a `HostPtr`:
///
/// 1. **Lifetime**: The pointed-to `InteropHost` must outlive the `HostPtr` (and therefore
///    the `ExecutionEngine` that holds it).
/// 2. **Exclusive access**: While the `ExecutionEngine` holds this pointer, no other code
///    should hold a mutable reference to the same `InteropHost`.
/// 3. **Single-thread access**: The `ExecutionEngine` is not shared across threads
///    concurrently. `HostPtr` implements `Send` so the engine can be *moved* between
///    threads, but it is deliberately `!Sync` because the raw pointer must not be
///    dereferenced from multiple threads simultaneously.
/// 4. **Validity**: The pointer must not be dangling. The `Option` wrapper on the engine
///    field handles the null case.
///
/// `HostPtr` is `Copy` because it wraps a raw pointer -- this is required so that it can
/// be extracted from `&self` before passing `&mut self` to the host callback methods
/// (mirroring the original raw host pointer which was also `Copy`).
pub(crate) struct HostPtr<S = ()>(
    NonNull<()>,
    HostCallbacks<S>,
    bool,
    /// Marker to make `HostPtr` `!Send` and `!Sync` by default so that the
    /// manual `Send` impl below is the only path to thread-safety.
    std::marker::PhantomData<*const ()>,
);

impl<S> Copy for HostPtr<S> {}

impl<S> Clone for HostPtr<S> {
    fn clone(&self) -> Self {
        *self
    }
}

// SAFETY: `ExecutionEngine` (the sole owner of `HostPtr`) is never shared
// across threads (`!Sync`). Sending the engine to another thread is safe
// because the pointed-to host moves with it (the host is the parent struct
// that owns the engine). All mutable access is serialized through
// `&mut ExecutionEngine`.
// Rationale: raw host pointers are confined to this VM interop bridge so the
// execution engine can avoid per-callback dynamic ownership wrappers.
#[allow(unsafe_code)]
unsafe impl<S> Send for HostPtr<S> {}

impl<S> HostPtr<S> {
    /// Creates a new `HostPtr` from a raw pointer.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that `ptr` is valid for the lifetime of this `HostPtr`
    /// and that no aliasing `&mut` references exist during method calls.
    // Rationale: creating the raw host wrapper is the single unsafe entry point
    // for the VM interop callback fast path.
    #[allow(unsafe_code)]
    pub(crate) unsafe fn new<H: InteropHost<S>>(ptr: *mut H) -> Self {
        let ptr = NonNull::new(ptr).expect("interop host pointer must not be null");
        // SAFETY: the constructor contract guarantees a valid, exclusively
        // accessible H pointer while the host binding exists.
        let post_execute_instruction_enabled =
            unsafe { ptr.as_ref() }.post_execute_instruction_enabled();
        Self(
            ptr.cast::<()>(),
            HostCallbacks::for_host::<H>(),
            post_execute_instruction_enabled,
            std::marker::PhantomData,
        )
    }

    /// Returns the underlying raw pointer (for API compatibility with callers that
    /// need to pass it onward).
    #[inline]
    pub(crate) fn as_raw(&self) -> *mut () {
        self.0.as_ptr()
    }

    /// Returns whether the attached host requested post-instruction callbacks.
    #[inline]
    pub(crate) const fn post_execute_instruction_enabled(&self) -> bool {
        self.2
    }

    /// Calls [`InteropHost::on_context_loaded`] on the wrapped host.
    ///
    /// # Safety (internal)
    ///
    /// Safe to call as long as the `HostPtr` invariants documented on the type are upheld.
    // Rationale: callbacks stay allocation-free by using the proven host
    // pointer invariant instead of boxing every VM host transition.
    #[allow(unsafe_code)]
    pub(crate) fn on_context_loaded(
        &self,
        engine: &mut ExecutionEngine<S>,
        context: &ExecutionContext<S>,
    ) -> VmResult<()> {
        // SAFETY: Invariant maintained by constructor contract -- the pointer is valid
        // and exclusively accessible for the duration of this call.
        unsafe { (self.1.on_context_loaded)(self.0, engine, context) }
    }

    /// Calls [`InteropHost::on_context_unloaded`] on the wrapped host.
    // Rationale: callbacks stay allocation-free by using the proven host
    // pointer invariant instead of boxing every VM host transition.
    #[allow(unsafe_code)]
    pub(crate) fn on_context_unloaded(
        &self,
        engine: &mut ExecutionEngine<S>,
        context: &ExecutionContext<S>,
    ) -> VmResult<()> {
        // SAFETY: Invariant maintained by constructor contract -- the pointer is valid
        // and exclusively accessible for the duration of this call.
        unsafe { (self.1.on_context_unloaded)(self.0, engine, context) }
    }

    /// Calls [`InteropHost::pre_execute_instruction`] on the wrapped host.
    // Rationale: instruction hooks are on the VM hot path and use the confined
    // host pointer invariant to avoid dispatch wrapper allocation.
    #[allow(unsafe_code)]
    pub(crate) fn pre_execute_instruction(
        &self,
        engine: &mut ExecutionEngine<S>,
        instruction: &Instruction,
    ) -> VmResult<()> {
        // SAFETY: Invariant maintained by constructor contract -- the pointer is valid
        // and exclusively accessible for the duration of this call.
        unsafe { (self.1.pre_execute_instruction)(self.0, engine, instruction) }
    }

    /// Calls [`InteropHost::post_execute_instruction`] on the wrapped host.
    // Rationale: instruction hooks are on the VM hot path and use the confined
    // host pointer invariant to avoid dispatch wrapper allocation.
    #[allow(unsafe_code)]
    pub(crate) fn post_execute_instruction(
        &self,
        engine: &mut ExecutionEngine<S>,
        instruction: &Instruction,
    ) -> VmResult<()> {
        // SAFETY: Invariant maintained by constructor contract -- the pointer is valid
        // and exclusively accessible for the duration of this call.
        unsafe { (self.1.post_execute_instruction)(self.0, engine, instruction) }
    }

    /// Calls [`InteropHost::invoke_syscall`] on the wrapped host.
    // Rationale: syscall dispatch is a VM hot path and uses the confined host
    // pointer invariant to avoid an additional ownership layer.
    #[allow(unsafe_code)]
    pub(crate) fn invoke_syscall(
        &self,
        engine: &mut ExecutionEngine<S>,
        hash: u32,
    ) -> VmResult<()> {
        // SAFETY: Invariant maintained by constructor contract -- the pointer is valid
        // and exclusively accessible for the duration of this call.
        unsafe { (self.1.invoke_syscall)(self.0, engine, hash) }
    }

    /// Calls [`InteropHost::on_callt`] on the wrapped host.
    // Rationale: CALLT dispatch is a VM hot path and uses the confined host
    // pointer invariant to avoid an additional ownership layer.
    #[allow(unsafe_code)]
    pub(crate) fn on_callt(&self, engine: &mut ExecutionEngine<S>, token_id: u16) -> VmResult<()> {
        // SAFETY: Invariant maintained by constructor contract -- the pointer is valid
        // and exclusively accessible for the duration of this call.
        unsafe { (self.1.on_callt)(self.0, engine, token_id) }
    }
}

#[allow(unsafe_code)]
fn host_ptr<H: InteropHost<S>, S>(ptr: NonNull<()>) -> *mut H {
    ptr.as_ptr().cast::<H>()
}

#[allow(unsafe_code)]
unsafe fn on_context_loaded<H: InteropHost<S>, S>(
    ptr: NonNull<()>,
    engine: &mut ExecutionEngine<S>,
    context: &ExecutionContext<S>,
) -> VmResult<()> {
    // SAFETY: `HostPtr::new` stores a pointer created from `*mut H` together
    // with these `H`-monomorphized callbacks. The caller upholds validity and
    // exclusivity for each callback invocation.
    unsafe { &mut *host_ptr::<H, S>(ptr) }.on_context_loaded(engine, context)
}

#[allow(unsafe_code)]
unsafe fn on_context_unloaded<H: InteropHost<S>, S>(
    ptr: NonNull<()>,
    engine: &mut ExecutionEngine<S>,
    context: &ExecutionContext<S>,
) -> VmResult<()> {
    // SAFETY: see `on_context_loaded`.
    unsafe { &mut *host_ptr::<H, S>(ptr) }.on_context_unloaded(engine, context)
}

#[allow(unsafe_code)]
unsafe fn pre_execute_instruction<H: InteropHost<S>, S>(
    ptr: NonNull<()>,
    engine: &mut ExecutionEngine<S>,
    instruction: &Instruction,
) -> VmResult<()> {
    // SAFETY: see `on_context_loaded`.
    unsafe { &mut *host_ptr::<H, S>(ptr) }.pre_execute_instruction(engine, instruction)
}

#[allow(unsafe_code)]
unsafe fn post_execute_instruction<H: InteropHost<S>, S>(
    ptr: NonNull<()>,
    engine: &mut ExecutionEngine<S>,
    instruction: &Instruction,
) -> VmResult<()> {
    // SAFETY: see `on_context_loaded`.
    unsafe { &mut *host_ptr::<H, S>(ptr) }.post_execute_instruction(engine, instruction)
}

#[allow(unsafe_code)]
unsafe fn invoke_syscall<H: InteropHost<S>, S>(
    ptr: NonNull<()>,
    engine: &mut ExecutionEngine<S>,
    hash: u32,
) -> VmResult<()> {
    // SAFETY: see `on_context_loaded`.
    unsafe { &mut *host_ptr::<H, S>(ptr) }.invoke_syscall(engine, hash)
}

#[allow(unsafe_code)]
unsafe fn on_callt<H: InteropHost<S>, S>(
    ptr: NonNull<()>,
    engine: &mut ExecutionEngine<S>,
    token_id: u16,
) -> VmResult<()> {
    // SAFETY: see `on_context_loaded`.
    unsafe { &mut *host_ptr::<H, S>(ptr) }.on_callt(engine, token_id)
}
