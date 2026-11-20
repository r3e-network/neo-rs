// Copyright (c) 2024 R3E Network

//! Shared states for execution contexts that can be cloned and shared.
//! This matches the C# implementation's `SharedStates` class exactly.

use crate::evaluation_stack::EvaluationStack;
use crate::reference_counter::ReferenceCounter;
use crate::script::Script;
use crate::slot::Slot;
use crate::stack_item::StackItem;
use parking_lot::{Mutex, RwLock};
use std::any::{Any, TypeId};
use std::collections::HashMap;
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
#[derive(Clone)]
pub struct SharedStates {
    /// Script being executed
    script: Arc<Script>,
    /// Evaluation stack for this context (shared across CALL clones; see struct doc)
    evaluation_stack: Arc<Mutex<EvaluationStack>>,
    /// Static fields shared across all clones
    static_fields: Arc<Mutex<Option<Slot>>>,
    /// Reference counter for garbage collection
    reference_counter: ReferenceCounter,
    /// State map matching C# Dictionary<Type, object>
    states: Arc<RwLock<HashMap<TypeId, Box<dyn Any + Send + Sync>>>>,
}

impl std::fmt::Debug for SharedStates {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedStates")
            .field("script", &"<Script>")
            .field("evaluation_stack", &"<SharedEvaluationStack>")
            .field("static_fields", &"<SharedStaticFields>")
            .finish()
    }
}

impl SharedStates {
    /// Creates a new shared states instance.
    #[must_use]
    pub fn new(script: Script, reference_counter: ReferenceCounter) -> Self {
        Self::new_with_stack_buffer(script, reference_counter, None)
    }

    /// Creates a new shared states instance, optionally reusing a recycled
    /// evaluation-stack backing allocation.
    ///
    /// `stack_buffer` is the storage the new evaluation stack starts with; when
    /// it is `None` a fresh allocation is made. Callers only pass a buffer that
    /// [`Self::take_exclusive_stack_buffer`] produced, so it is empty and no
    /// surrounding state has ever observed it.
    #[must_use]
    pub fn new_with_stack_buffer(
        script: Script,
        reference_counter: ReferenceCounter,
        stack_buffer: Option<Vec<StackItem>>,
    ) -> Self {
        let script = Arc::new(script);
        let evaluation_stack = match stack_buffer {
            Some(buffer) => EvaluationStack::with_backing_buffer(reference_counter.clone(), buffer),
            None => EvaluationStack::new(reference_counter.clone()),
        };
        Self {
            script: Arc::clone(&script),
            evaluation_stack: Arc::new(Mutex::new(evaluation_stack)),
            static_fields: Arc::new(Mutex::new(None)),
            reference_counter,
            states: Arc::new(RwLock::new(HashMap::new())),
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

    /// Detaches the evaluation stack's backing allocation when, and only when,
    /// this instance is its exclusive owner.
    ///
    /// `Arc::get_mut` succeeds only with a strong count of 1 and no weak
    /// handles. `ExecutionContext::clone_with_position` - the CALL path -
    /// upgrades this handle into a second `Arc`, so while a caller, a callee or
    /// any other shared context still aliases the stack this returns `None` and
    /// the allocation stays where it is. The evaluation stack itself is left in
    /// place and simply starts without storage.
    pub fn take_exclusive_stack_buffer(&mut self) -> Option<Vec<StackItem>> {
        let stack = Arc::get_mut(&mut self.evaluation_stack)?.get_mut();
        Some(stack.take_backing_buffer())
    }

    /// Executes a closure with mutable access to the shared static fields.
    pub fn with_static_fields_mut<R, F>(&self, f: F) -> R
    where
        F: FnOnce(&mut Option<Slot>) -> R,
    {
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

    /// Sets the static fields.
    pub fn set_static_fields(&mut self, static_fields: Option<Slot>) {
        *self.static_fields.lock() = static_fields;
    }

    /// Gets custom data of the specified type. If the data does not exist, create a new one.
    /// Mirrors the C# SharedStates.GetState\<T>() behaviour by caching instances per type.
    pub fn get_state<T>(&self) -> Arc<Mutex<T>>
    where
        T: 'static + Default + Send + Sync,
    {
        self.get_state_with_factory(T::default)
    }

    /// Gets custom data of the specified type, creating it with the provided factory if it doesn't exist.
    /// Mirrors the C# SharedStates.GetState\<T>(Func\<T>) API.
    pub fn get_state_with_factory<T, F>(&self, factory: F) -> Arc<Mutex<T>>
    where
        T: 'static + Send + Sync,
        F: FnOnce() -> T,
    {
        let type_id = TypeId::of::<T>();

        if let Some(existing) = self.states.read().get(&type_id)
            && let Some(arc) = existing.downcast_ref::<Arc<Mutex<T>>>()
        {
            return Arc::clone(arc);
        }

        let mut states = self.states.write();
        if let Some(existing) = states.get(&type_id)
            && let Some(arc) = existing.downcast_ref::<Arc<Mutex<T>>>()
        {
            return Arc::clone(arc);
        }

        let new_state = Arc::new(Mutex::new(factory()));
        states.insert(type_id, Box::new(Arc::clone(&new_state)));
        new_state
    }

    /// Sets a state value by type.
    /// This matches the C# implementation's state setting behavior.
    pub fn set_state<T>(&self, value: T)
    where
        T: 'static + Send + Sync,
    {
        let type_id = TypeId::of::<T>();
        let state = Arc::new(Mutex::new(value));

        let mut states = self.states.write();
        states.insert(type_id, Box::new(state));
    }
}
