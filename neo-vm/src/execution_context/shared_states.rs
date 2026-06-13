// Copyright (c) 2024 R3E Network

//! Shared states for execution contexts that can be cloned and shared.
//! This matches the C# implementation's `SharedStates` class exactly.

use crate::evaluation_stack::EvaluationStack;
use crate::reference_counter::ReferenceCounter;
use crate::script::Script;
use crate::slot::Slot;
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
        let script = Arc::new(script);
        Self {
            script: Arc::clone(&script),
            evaluation_stack: Arc::new(Mutex::new(EvaluationStack::new(reference_counter.clone()))),
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

    /// Sets the static fields.
    pub fn set_static_fields(&mut self, static_fields: Option<Slot>) {
        *self.static_fields.lock() = static_fields;
    }

    /// Gets custom data of the specified type. If the data does not exist, create a new one.
    /// Mirrors the C# SharedStates.GetState\<T>() behaviour by caching instances per type.
    pub fn get_state<T: 'static + Default + Send + Sync>(&self) -> Arc<Mutex<T>> {
        self.get_state_with_factory(T::default)
    }

    /// Gets custom data of the specified type, creating it with the provided factory if it doesn't exist.
    /// Mirrors the C# SharedStates.GetState\<T>(Func\<T>) API.
    pub fn get_state_with_factory<T: 'static + Send + Sync, F: FnOnce() -> T>(
        &self,
        factory: F,
    ) -> Arc<Mutex<T>> {
        let type_id = TypeId::of::<T>();

        if let Some(existing) = self.states.read().get(&type_id) {
            if let Some(arc) = existing.downcast_ref::<Arc<Mutex<T>>>() {
                return Arc::clone(arc);
            }
        }

        let mut states = self.states.write();
        if let Some(existing) = states.get(&type_id) {
            if let Some(arc) = existing.downcast_ref::<Arc<Mutex<T>>>() {
                return Arc::clone(arc);
            }
        }

        let new_state = Arc::new(Mutex::new(factory()));
        states.insert(type_id, Box::new(Arc::clone(&new_state)));
        new_state
    }

    /// Sets a state value by type.
    /// This matches the C# implementation's state setting behavior.
    pub fn set_state<T: 'static + Send + Sync>(&self, value: T) {
        let type_id = TypeId::of::<T>();
        let state = Arc::new(Mutex::new(value));

        let mut states = self.states.write();
        states.insert(type_id, Box::new(state));
    }
}
