// Copyright (c) 2024 R3E Network

//! Execution context module for the Neo Virtual Machine.
//!
//! This module provides the execution context implementation for the Neo VM.

use super::shared_states::SharedStates;
use crate::ExceptionHandlingContext;
use crate::Instruction;
use crate::error::VmError;
use crate::error::VmResult;
use crate::evaluation_stack::EvaluationStack;
use crate::reference_counter::ReferenceCounter;
use crate::script::Script;
use crate::stack_item::StackItem;
use neo_crypto::Crypto;
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// A slot for storing variables or arguments in a context.
///
/// This is a thin alias over [`crate::slot::Slot`] so existing call sites
/// can continue to reference `execution_context::Slot`.
pub type Slot = crate::slot::Slot;

static NEXT_CONTEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_context_id() -> u64 {
    NEXT_CONTEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Represents an execution context in the Neo Virtual Machine.
/// This matches the C# implementation's `ExecutionContext` class.
pub struct ExecutionContext {
    /// Identity of this context object. Clones receive a new id. The id is never
    /// reused, so callers can cache facts about one context for its lifetime.
    id: u64,
    /// The shared states (script, evaluation stack, static fields)
    shared_states: SharedStates,

    /// The current instruction pointer
    instruction_pointer: usize,

    /// The number of values to return when the context is unloaded (-1 for all)
    rvcount: i32,

    /// The local variables for this context
    local_variables: Option<Slot>,

    /// The arguments for this context
    arguments: Option<Slot>,

    /// The stack containing nested exception handling contexts
    try_stack: Option<Vec<ExceptionHandlingContext>>,
}

impl ExecutionContext {
    /// Creates a new execution context.
    /// This matches the C# implementation's constructor pattern.
    #[must_use]
    pub fn new(script: Script, rvcount: i32, reference_counter: &ReferenceCounter) -> Self {
        Self::new_with_stack_buffer(script, rvcount, reference_counter, None)
    }

    /// Creates a new execution context, optionally reusing a recycled
    /// evaluation-stack backing allocation.
    ///
    /// A `stack_buffer` is only ever supplied by the engine, which takes it
    /// from its [`StackAllocator`](crate::memory::StackAllocator). It must be
    /// empty: a brand new context holds no stack references, so it cannot
    /// adopt storage that still contains entries.
    #[must_use]
    pub fn new_with_stack_buffer(
        script: Script,
        rvcount: i32,
        reference_counter: &ReferenceCounter,
        stack_buffer: Option<Vec<StackItem>>,
    ) -> Self {
        Self {
            id: next_context_id(),
            shared_states: SharedStates::new_with_stack_buffer(
                script,
                reference_counter.clone(),
                stack_buffer,
            ),
            instruction_pointer: 0,
            rvcount,
            local_variables: None,
            arguments: None,
            try_stack: None,
        }
    }

    /// Detaches this context's evaluation-stack backing allocation when the
    /// context is its exclusive owner.
    ///
    /// Returns `None` while any other shared context still aliases the stack,
    /// which is exactly the `CALL` case. The caller must be about to dispose of
    /// the context: the entries leave with the allocation and are released when
    /// it is dropped, as they would be when the context itself was dropped.
    pub fn take_exclusive_evaluation_buffer(&mut self) -> Option<Vec<StackItem>> {
        self.shared_states.take_exclusive_stack_buffer()
    }

    /// Returns the unique id assigned when this context object was created.
    #[inline]
    #[must_use]
    pub fn context_id(&self) -> u64 {
        self.id
    }

    /// Returns the script for this context.
    /// This matches the C# implementation's Script property.
    #[must_use]
    pub fn script(&self) -> &Script {
        self.shared_states.script()
    }

    /// Returns the script as an Arc for identity-sensitive operations (matches C# reference semantics).
    #[must_use]
    pub fn script_arc(&self) -> Arc<Script> {
        self.shared_states.script_arc()
    }

    /// Returns the reference counter associated with this context.
    #[must_use]
    pub fn reference_counter(&self) -> &ReferenceCounter {
        self.shared_states.reference_counter()
    }

    /// Returns the script hash for this context as a 20-byte array.
    /// This mirrors the C# `Script.ToScriptHash()` behaviour (Hash160).
    #[must_use]
    pub fn script_hash(&self) -> [u8; 20] {
        Crypto::hash160(self.script().as_bytes())
    }

    /// Returns the current instruction pointer.
    #[inline]
    #[must_use]
    pub const fn instruction_pointer(&self) -> usize {
        self.instruction_pointer
    }

    /// Sets the instruction pointer.
    #[inline]
    pub fn set_instruction_pointer(&mut self, position: usize) {
        self.instruction_pointer = position;
    }

    /// Returns the current instruction or None if at the end of the script.
    /// This matches the C# implementation's `CurrentInstruction` property.
    #[inline]
    pub fn current_instruction(&self) -> VmResult<std::sync::Arc<Instruction>> {
        let ip = self.instruction_pointer;
        let script = self.script();
        if ip >= script.len() {
            return Err(VmError::invalid_operation_msg(
                "Instruction pointer is out of range",
            ));
        }
        script.get_instruction(ip)
    }

    /// Returns the next instruction or None if at the end of the script.
    /// This matches the C# implementation's `NextInstruction` property.
    pub fn next_instruction(&self) -> VmResult<std::sync::Arc<Instruction>> {
        let current = self.current_instruction()?;
        let next_position = self.instruction_pointer + current.size();

        if next_position >= self.script().len() {
            return Err(VmError::invalid_operation_msg(
                "Next instruction is out of range",
            ));
        }

        self.script().get_instruction(next_position)
    }

    /// Returns the number of values to return when the context is unloaded (-1 for all).
    #[must_use]
    pub const fn rvcount(&self) -> i32 {
        self.rvcount
    }

    /// Sets the return value count for the context.
    pub fn set_rvcount(&mut self, rvcount: i32) {
        self.rvcount = rvcount;
    }

    /// Returns the evaluation stack for this context.
    /// This matches the C# implementation's `EvaluationStack` property.
    pub fn evaluation_stack(&self) -> parking_lot::MutexGuard<'_, EvaluationStack> {
        self.shared_states.evaluation_stack()
    }

    /// Returns the evaluation stack for this context (mutable).
    /// This matches the C# implementation's `EvaluationStack` property.
    pub fn evaluation_stack_mut(&self) -> parking_lot::MutexGuard<'_, EvaluationStack> {
        self.shared_states.evaluation_stack_mut()
    }

    /// Returns true when static fields are initialized for this context.
    #[must_use]
    pub fn has_static_fields(&self) -> bool {
        self.shared_states.has_static_fields()
    }

    /// Executes a closure with mutable access to static fields.
    pub fn with_static_fields_mut<R, F>(&self, f: F) -> R
    where
        F: FnOnce(&mut Option<Slot>) -> R,
    {
        self.shared_states.with_static_fields_mut(f)
    }

    /// Sets the static fields for this context.
    pub fn set_static_fields(&mut self, static_fields: Option<Slot>) {
        self.shared_states.set_static_fields(static_fields);
    }

    /// Returns true when both contexts share the same evaluation stack.
    #[must_use]
    pub fn shares_evaluation_stack_with(&self, other: &Self) -> bool {
        self.shared_states
            .evaluation_stack_ptr_eq(&other.shared_states)
    }

    /// Returns true when both contexts share the same static field slot.
    #[must_use]
    pub fn shares_static_fields_with(&self, other: &Self) -> bool {
        self.shared_states
            .static_fields_ptr_eq(&other.shared_states)
    }

    /// Clears static field references for this context.
    pub fn clear_static_fields_references(&self) {
        self.with_static_fields_mut(|static_fields| {
            if let Some(static_fields) = static_fields.as_mut() {
                static_fields.clear_references();
            }
        });
    }

    /// Returns the number of initialized static fields in this context.
    #[must_use]
    pub fn static_fields_len(&self) -> usize {
        self.with_static_fields_mut(|static_fields| {
            static_fields.as_ref().map_or(0, crate::slot::Slot::len)
        })
    }

    /// Returns the local variables for this context.
    #[must_use]
    pub const fn local_variables(&self) -> Option<&Slot> {
        self.local_variables.as_ref()
    }

    /// Returns the local variables for this context (mutable).
    pub fn local_variables_mut(&mut self) -> Option<&mut Slot> {
        self.local_variables.as_mut()
    }

    /// Sets the local variables for this context.
    pub fn set_local_variables(&mut self, local_variables: Option<Slot>) {
        self.local_variables = local_variables;
    }

    /// Returns the arguments for this context.
    #[must_use]
    pub const fn arguments(&self) -> Option<&Slot> {
        self.arguments.as_ref()
    }

    /// Returns the arguments for this context (mutable).
    pub fn arguments_mut(&mut self) -> Option<&mut Slot> {
        self.arguments.as_mut()
    }

    /// Sets the arguments for this context.
    pub fn set_arguments(&mut self, arguments: Option<Slot>) {
        self.arguments = arguments;
    }

    /// Returns the try stack for this context.
    #[must_use]
    pub const fn try_stack(&self) -> Option<&Vec<ExceptionHandlingContext>> {
        self.try_stack.as_ref()
    }

    /// Returns the try stack for this context (mutable).
    pub fn try_stack_mut(&mut self) -> Option<&mut Vec<ExceptionHandlingContext>> {
        self.try_stack.as_mut()
    }

    /// Sets the try stack for this context.
    pub fn set_try_stack(&mut self, try_stack: Option<Vec<ExceptionHandlingContext>>) {
        self.try_stack = try_stack;
    }

    /// Returns the number of nested try contexts currently tracked.
    #[must_use]
    pub fn try_stack_len(&self) -> usize {
        self.try_stack.as_ref().map_or(0, std::vec::Vec::len)
    }

    /// Returns true when at least one try context is active.
    #[must_use]
    pub fn has_try_context(&self) -> bool {
        self.try_stack_len() > 0
    }

    /// Pushes a new try context onto the stack, initialising the stack if needed.
    pub fn push_try_context(&mut self, ctx: ExceptionHandlingContext) {
        self.try_stack.get_or_insert_with(Vec::new).push(ctx);
    }

    /// Pops the current try context if one exists.
    pub fn pop_try_context(&mut self) -> Option<ExceptionHandlingContext> {
        self.try_stack.as_mut().and_then(std::vec::Vec::pop)
    }

    /// Returns the current try context if one exists.
    #[must_use]
    pub fn try_stack_last(&self) -> Option<&ExceptionHandlingContext> {
        self.try_stack.as_ref().and_then(|stack| stack.last())
    }

    /// Returns a mutable handle to the current try context if one exists.
    pub fn try_stack_last_mut(&mut self) -> Option<&mut ExceptionHandlingContext> {
        self.try_stack.as_mut().and_then(|stack| stack.last_mut())
    }

    /// Moves to the next instruction.
    pub fn move_next(&mut self) -> VmResult<()> {
        if self.instruction_pointer >= self.script().len() {
            return Err(VmError::invalid_operation_msg(
                "Instruction pointer is out of range",
            ));
        }

        let instruction = self.script().get_instruction(self.instruction_pointer)?;
        self.instruction_pointer += instruction.size();

        Ok(())
    }

    /// Advances the instruction pointer by the given size without re-fetching
    /// the instruction. Use when the caller already knows the instruction size.
    #[inline]
    pub fn advance_ip(&mut self, instruction_size: usize) {
        self.instruction_pointer += instruction_size;
    }

    /// Gets a state value for the specified type, creating it if it doesn't exist.
    /// Mirrors the C# ExecutionContext.GetState\<T>() helper.
    #[must_use]
    pub fn get_state<T>(&self) -> Arc<Mutex<T>>
    where
        T: 'static + Default + Send + Sync,
    {
        self.shared_states.get_state::<T>()
    }

    /// Gets a state value for the specified type using the provided factory when absent.
    /// Mirrors the C# ExecutionContext.GetState\<T>(Func\<T>) helper.
    pub fn get_state_with_factory<T, F>(&self, factory: F) -> Arc<Mutex<T>>
    where
        T: 'static + Send + Sync,
        F: FnOnce() -> T,
    {
        self.shared_states.get_state_with_factory(factory)
    }

    /// Sets a state value by key.
    /// This matches the C# implementation's state setting behavior.
    pub fn set_state<T>(&self, value: T)
    where
        T: 'static + Send + Sync,
    {
        self.shared_states.set_state(value);
    }

    /// Push an item onto the evaluation stack
    #[inline(always)]
    pub fn push(&mut self, item: crate::stack_item::StackItem) -> VmResult<()> {
        self.shared_states.evaluation_stack_mut().push(item)
    }

    /// Pop an item from the evaluation stack
    #[inline(always)]
    pub fn pop(&mut self) -> VmResult<crate::stack_item::StackItem> {
        self.shared_states.evaluation_stack_mut().pop()
    }

    /// Pop an item without releasing its stack reference.
    ///
    /// neo-vm v3.11.2 `EvaluationStack.PopNoRef`.
    #[inline(always)]
    pub fn pop_no_ref(&mut self) -> VmResult<crate::stack_item::StackItem> {
        self.shared_states.evaluation_stack_mut().pop_no_ref()
    }

    /// Push an item and add exactly `count` to the reference counter.
    ///
    /// neo-vm v3.11.2 `EvaluationStack.PushItemCounted`.
    #[inline(always)]
    pub fn push_item_counted(&mut self, item: crate::stack_item::StackItem, count: i64) {
        self.shared_states
            .evaluation_stack_mut()
            .push_item_counted(item, count);
    }

    /// Move the item `index` places below the top of the stack to the top.
    ///
    /// neo-vm v3.11.2 `EvaluationStack.Roll`.
    #[inline(always)]
    pub fn roll(&mut self, index: usize) -> VmResult<()> {
        self.shared_states.evaluation_stack_mut().roll(index)
    }

    /// Peek at an item on the evaluation stack without removing it
    #[inline(always)]
    pub fn peek(&self, index: usize) -> VmResult<crate::stack_item::StackItem> {
        self.shared_states.evaluation_stack().peek(index).cloned()
    }

    /// Insert an item at the specified index from the top of the evaluation stack.
    #[inline]
    pub fn insert(
        &mut self,
        index_from_top: usize,
        item: crate::stack_item::StackItem,
    ) -> VmResult<()> {
        self.shared_states
            .evaluation_stack_mut()
            .insert(index_from_top, item)
    }

    /// Clones the context with a new reference counter.
    pub fn clone_for_reference_counter(
        &self,
        reference_counter: &ReferenceCounter,
    ) -> VmResult<Self> {
        // Create a new shared states with the new reference counter
        let shared_states = SharedStates::new(self.script().clone(), reference_counter.clone());

        // Copy the evaluation stack. The entries are pushed so that the new
        // reference counter accounts for them.
        {
            let source_stack = self.evaluation_stack();
            let mut target_stack = shared_states.evaluation_stack_mut();
            for item in source_stack.iter() {
                target_stack.push(item.clone())?;
            }
        }

        // Create the new context

        // Note: Not cloning static_fields, local_variables, or arguments
        // as they would need separate reference counter handling

        Ok(Self {
            id: next_context_id(),
            shared_states,
            instruction_pointer: self.instruction_pointer,
            rvcount: self.rvcount,
            local_variables: None,
            arguments: None,
            try_stack: self.try_stack.clone(),
        })
    }

    /// Clones the context for a CALL operation.
    /// Matches C# `ExecutionContext.Clone(position)`: shared script/evaluation stack/static fields
    /// and `rvcount = 0`.
    #[must_use]
    pub fn clone_with_position(&self, position: usize) -> Self {
        Self {
            id: next_context_id(),
            shared_states: self.shared_states.clone(),
            instruction_pointer: position,
            rvcount: 0,
            local_variables: None,
            arguments: None,
            try_stack: None,
        }
    }

    /// Gets a shared state by type, creating it if it doesn't exist.
    /// Mirrors the C# ExecutionContext.GetState\<T>() helper for shared state access.
    #[must_use]
    pub fn get_shared_state<T>(&self) -> Arc<Mutex<T>>
    where
        T: 'static + Default + Send + Sync,
    {
        self.shared_states.get_state::<T>()
    }

    /// Gets a shared state by type, creating it with the provided factory if it doesn't exist.
    pub fn get_shared_state_with_factory<T, F>(&self, factory: F) -> Arc<Mutex<T>>
    where
        T: 'static + Send + Sync,
        F: FnOnce() -> T,
    {
        self.shared_states.get_state_with_factory(factory)
    }

    /// Initializes the slots for local variables and arguments.
    pub fn init_slot(&mut self, local_count: usize, argument_count: usize) -> VmResult<()> {
        // Initialize local variables
        if local_count > 0 {
            let reference_counter = self.shared_states.reference_counter().clone();
            self.local_variables = Some(Slot::with_capacity(local_count, reference_counter));
        }

        // Initialize arguments
        if argument_count > 0 {
            let reference_counter = self.shared_states.reference_counter().clone();
            self.arguments = Some(Slot::with_capacity(argument_count, reference_counter));
        }

        Ok(())
    }

    /// Loads a value from a static field.
    pub fn load_static_field(&self, index: usize) -> VmResult<crate::stack_item::StackItem> {
        self.shared_states.with_static_fields_mut(|static_fields| {
            if let Some(static_fields) = static_fields.as_ref() {
                static_fields.get(index).cloned().ok_or_else(|| {
                    VmError::invalid_operation_msg("Static field index out of range")
                })
            } else {
                Err(VmError::invalid_operation_msg(
                    "No static fields initialized",
                ))
            }
        })
    }

    /// Stores a value to a static field.
    pub fn store_static_field(
        &mut self,
        index: usize,
        value: crate::stack_item::StackItem,
    ) -> VmResult<()> {
        self.shared_states.with_static_fields_mut(|static_fields| {
            if let Some(static_fields) = static_fields.as_mut() {
                static_fields.set(index, value)?;
                Ok(())
            } else {
                Err(VmError::invalid_operation_msg(
                    "No static fields initialized",
                ))
            }
        })
    }

    /// Loads a value from a local variable.
    pub fn load_local(&self, index: usize) -> VmResult<crate::stack_item::StackItem> {
        if let Some(local_variables) = &self.local_variables {
            local_variables
                .get(index)
                .cloned()
                .ok_or_else(|| VmError::invalid_operation_msg("Local variable index out of range"))
        } else {
            Err(VmError::invalid_operation_msg(
                "No local variables initialized",
            ))
        }
    }

    /// Stores a value to a local variable.
    pub fn store_local(
        &mut self,
        index: usize,
        value: crate::stack_item::StackItem,
    ) -> VmResult<()> {
        if let Some(local_variables) = &mut self.local_variables {
            local_variables.set(index, value)?;
            Ok(())
        } else {
            Err(VmError::invalid_operation_msg(
                "No local variables initialized",
            ))
        }
    }

    /// Loads a value from an argument.
    pub fn load_argument(&self, index: usize) -> VmResult<crate::stack_item::StackItem> {
        if let Some(arguments) = &self.arguments {
            arguments
                .get(index)
                .cloned()
                .ok_or_else(|| VmError::invalid_operation_msg("Argument index out of range"))
        } else {
            Err(VmError::invalid_operation_msg("No arguments initialized"))
        }
    }

    /// Stores a value to an argument.
    pub fn store_argument(
        &mut self,
        index: usize,
        value: crate::stack_item::StackItem,
    ) -> VmResult<()> {
        if let Some(arguments) = &mut self.arguments {
            arguments.set(index, value)?;
            Ok(())
        } else {
            Err(VmError::invalid_operation_msg("No arguments initialized"))
        }
    }

    /// Moves the top evaluation-stack item into the static field at `index`.
    ///
    /// neo-vm v3.11.2 `Slot.Store`: the item is popped via `PopNoRef` and keeps
    /// its stack reference, while the previous value's reference is released.
    /// The slot is resolved and bounds-checked before the pop, so a failed
    /// store leaves the evaluation stack untouched. The two-pass structure
    /// avoids holding the static-fields lock across the stack pop.
    pub fn store_static_field_from_stack(&mut self, index: usize) -> VmResult<()> {
        let slot_len = self.with_static_fields_mut(|static_fields| {
            static_fields
                .as_ref()
                .map(Slot::len)
                .ok_or_else(|| VmError::invalid_operation_msg("Slot has not been initialized."))
        })?;
        Slot::check_bounds(index, slot_len)?;
        let item = self.evaluation_stack_mut().pop_no_ref()?;
        self.with_static_fields_mut(|static_fields| {
            static_fields
                .as_mut()
                .expect("slot verified initialized")
                .store_popped(index, item);
        });
        Ok(())
    }

    /// Moves the top evaluation-stack item into the local variable at `index`.
    ///
    /// neo-vm v3.11.2 `Slot.Store` semantics; see `store_static_field_from_stack`.
    pub fn store_local_from_stack(&mut self, index: usize) -> VmResult<()> {
        let slot_len = self
            .local_variables
            .as_ref()
            .map(Slot::len)
            .ok_or_else(|| VmError::invalid_operation_msg("Slot has not been initialized."))?;
        Slot::check_bounds(index, slot_len)?;
        let item = self.evaluation_stack_mut().pop_no_ref()?;
        self.local_variables
            .as_mut()
            .expect("slot verified initialized")
            .store_popped(index, item);
        Ok(())
    }

    /// Moves the top evaluation-stack item into the argument at `index`.
    ///
    /// neo-vm v3.11.2 `Slot.Store` semantics; see `store_static_field_from_stack`.
    pub fn store_argument_from_stack(&mut self, index: usize) -> VmResult<()> {
        let slot_len = self
            .arguments
            .as_ref()
            .map(Slot::len)
            .ok_or_else(|| VmError::invalid_operation_msg("Slot has not been initialized."))?;
        Slot::check_bounds(index, slot_len)?;
        let item = self.evaluation_stack_mut().pop_no_ref()?;
        self.arguments
            .as_mut()
            .expect("slot verified initialized")
            .store_popped(index, item);
        Ok(())
    }
}

impl Clone for ExecutionContext {
    fn clone(&self) -> Self {
        self.clone_with_position(self.instruction_pointer)
    }
}

#[cfg(test)]
#[allow(dead_code)]
mod tests;
