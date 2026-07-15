// Copyright (c) 2024 R3E Network

//! Shared states for execution contexts that can be cloned and shared.
//! This matches the C# implementation's `SharedStates` class exactly.

use crate::evaluation_stack::EvaluationStack;
use crate::reference_counter::ReferenceCounter;
use crate::script::Script;
use crate::slot::Slot;
use parking_lot::Mutex;
use std::sync::Arc;

/// Shared states for execution contexts that can be cloned and shared.
/// This matches the C# implementation's `SharedStates` class exactly.
///
/// # Why `Arc<Mutex<EvaluationStack>>`?
///
/// The evaluation stack is wrapped in `Arc<Mutex<..>>` because:
/// 1. **Sharing**: CALL instructions clone `SharedStates` so that caller and
///    callee contexts share the same evaluation stack (see `clone_with_position`).
///    `Arc` is needed for this shared ownership.
/// 2. **Send + Sync**: `ExecutionEngine` (and `ApplicationEngine`) must be `Send`
///    to cross `.await` points in async/tokio code (see `neo-rpc` session handling).
///    Using `Rc<RefCell<..>>` would break `Send`.
/// 3. **Low overhead**: `parking_lot::Mutex` on an uncontended lock is a single
///    atomic CAS with no syscall, so the practical cost is negligible for the
///    single-threaded VM execution path.
pub struct SharedStates<S = ()> {
    /// Script being executed
    script: Arc<Script>,
    /// Evaluation stack for this context (shared across CALL clones; see struct doc)
    evaluation_stack: Arc<Mutex<EvaluationStack>>,
    /// Static fields shared across all clones
    static_fields: Arc<Mutex<Option<Slot>>>,
    /// Reference counter for garbage collection
    reference_counter: ReferenceCounter,
    /// Shared typed context state.
    state: Arc<Mutex<S>>,
}

impl<S> std::fmt::Debug for SharedStates<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedStates")
            .field("script", &"<Script>")
            .field("evaluation_stack", &"<SharedEvaluationStack>")
            .field("static_fields", &"<SharedStaticFields>")
            .field("state", &"<SharedState>")
            .finish()
    }
}

impl<S> Clone for SharedStates<S> {
    fn clone(&self) -> Self {
        Self {
            script: Arc::clone(&self.script),
            evaluation_stack: Arc::clone(&self.evaluation_stack),
            static_fields: Arc::clone(&self.static_fields),
            reference_counter: self.reference_counter.clone(),
            state: Arc::clone(&self.state),
        }
    }
}

impl<S: Default> SharedStates<S> {
    /// Creates a new shared states instance.
    #[must_use]
    pub fn new(script: Script, reference_counter: ReferenceCounter) -> Self {
        Self::new_with_state_factory(script, reference_counter, S::default)
    }

    /// Creates shared states that retain an existing script allocation.
    #[must_use]
    pub fn new_from_script_arc(script: Arc<Script>, reference_counter: ReferenceCounter) -> Self {
        Self {
            script,
            evaluation_stack: Arc::new(Mutex::new(EvaluationStack::new(reference_counter.clone()))),
            static_fields: Arc::new(Mutex::new(None)),
            reference_counter,
            state: Arc::new(Mutex::new(S::default())),
        }
    }
}

impl<S> SharedStates<S> {
    /// Creates a new shared states instance with an explicit typed state value.
    #[must_use]
    pub fn new_with_state(script: Script, reference_counter: ReferenceCounter, state: S) -> Self {
        Self::new_with_state_factory(script, reference_counter, || state)
    }

    /// Creates a new shared states instance with a typed-state factory.
    #[must_use]
    pub fn new_with_state_factory<F: FnOnce() -> S>(
        script: Script,
        reference_counter: ReferenceCounter,
        factory: F,
    ) -> Self {
        let script = Arc::new(script);
        Self {
            script: Arc::clone(&script),
            evaluation_stack: Arc::new(Mutex::new(EvaluationStack::new(reference_counter.clone()))),
            static_fields: Arc::new(Mutex::new(None)),
            reference_counter,
            state: Arc::new(Mutex::new(factory())),
        }
    }

    /// Returns the reference counter for this context.
    #[must_use]
    pub const fn reference_counter(&self) -> &ReferenceCounter {
        &self.reference_counter
    }

    /// Returns the script being executed.
    #[must_use]
    pub fn script(&self) -> &Script {
        self.script.as_ref()
    }

    /// Returns the script as an `Arc` for cloning.
    #[must_use]
    pub fn script_arc(&self) -> Arc<Script> {
        Arc::clone(&self.script)
    }

    /// Returns the evaluation stack.
    pub fn evaluation_stack(&self) -> parking_lot::MutexGuard<'_, EvaluationStack> {
        self.evaluation_stack.lock()
    }

    /// Returns a mutable reference to the evaluation stack.
    pub fn evaluation_stack_mut(&self) -> parking_lot::MutexGuard<'_, EvaluationStack> {
        self.evaluation_stack.lock()
    }

    /// Returns true when static fields are initialized.
    #[must_use]
    pub fn has_static_fields(&self) -> bool {
        self.static_fields.lock().is_some()
    }

    /// Executes a closure with mutable access to the shared static fields.
    pub fn with_static_fields_mut<R, F: FnOnce(&mut Option<Slot>) -> R>(&self, f: F) -> R {
        let mut guard = self.static_fields.lock();
        f(&mut guard)
    }

    /// Checks whether two shared state instances point to the same evaluation stack.
    #[must_use]
    pub fn evaluation_stack_ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.evaluation_stack, &other.evaluation_stack)
    }

    /// Checks whether two shared state instances point to the same static field slot.
    #[must_use]
    pub fn static_fields_ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.static_fields, &other.static_fields)
    }

    /// Returns the shared typed state.
    #[must_use]
    pub fn state(&self) -> Arc<Mutex<S>> {
        Arc::clone(&self.state)
    }

    /// Executes a closure with mutable access to the shared typed state.
    pub fn with_state<R, F: FnOnce(&mut S) -> R>(&self, f: F) -> R {
        let mut guard = self.state.lock();
        f(&mut guard)
    }

    /// Replaces the shared typed state value and returns the previous state.
    pub fn replace_state(&self, state: S) -> S {
        self.with_state(|current| std::mem::replace(current, state))
    }

    /// Checks whether two shared state instances point to the same typed state.
    #[must_use]
    pub fn state_ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }

    /// Sets the static fields.
    pub fn set_static_fields(&mut self, static_fields: Option<Slot>) {
        *self.static_fields.lock() = static_fields;
    }
}
