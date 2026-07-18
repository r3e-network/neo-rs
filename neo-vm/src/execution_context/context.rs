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
use crate::execution_plan::ExecutionPlan;
use crate::execution_profile::StackProfileHandle;
use crate::reference_counter::ReferenceCounter;
use crate::script::Script;
use parking_lot::Mutex;
use std::sync::Arc;

/// A slot for storing variables or arguments in a context.
///
/// This is a thin alias over [`crate::slot::Slot`] so existing call sites
/// can continue to reference `execution_context::Slot`.
pub type Slot = crate::slot::Slot;

/// Represents an execution context in the Neo Virtual Machine.
/// This matches the C# implementation's `ExecutionContext` class.
pub struct ExecutionContext<S = ()> {
    /// The shared states (script, evaluation stack, static fields)
    shared_states: SharedStates<S>,

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

impl<S: Default> ExecutionContext<S> {
    /// Creates a new execution context.
    /// This matches the C# implementation's constructor pattern.
    #[must_use]
    pub fn new(script: Script, rvcount: i32, reference_counter: &ReferenceCounter) -> Self {
        Self::new_with_state_factory(script, rvcount, reference_counter, S::default)
    }

    /// Creates an execution context that retains an existing script allocation.
    #[must_use]
    pub fn new_from_script_arc(
        script: Arc<Script>,
        rvcount: i32,
        reference_counter: &ReferenceCounter,
    ) -> Self {
        Self {
            shared_states: SharedStates::new_from_script_arc(script, reference_counter.clone()),
            instruction_pointer: 0,
            rvcount,
            local_variables: None,
            arguments: None,
            try_stack: None,
        }
    }
}

impl<S> ExecutionContext<S> {
    /// Creates a new execution context with an explicit typed state value.
    #[must_use]
    pub fn new_with_state(
        script: Script,
        rvcount: i32,
        reference_counter: &ReferenceCounter,
        state: S,
    ) -> Self {
        Self::new_with_state_factory(script, rvcount, reference_counter, || state)
    }

    /// Creates a new execution context with a typed-state factory.
    #[must_use]
    pub fn new_with_state_factory<F: FnOnce() -> S>(
        script: Script,
        rvcount: i32,
        reference_counter: &ReferenceCounter,
        factory: F,
    ) -> Self {
        Self {
            shared_states: SharedStates::new_with_state_factory(
                script,
                reference_counter.clone(),
                factory,
            ),
            instruction_pointer: 0,
            rvcount,
            local_variables: None,
            arguments: None,
            try_stack: None,
        }
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

    /// Returns the optional immutable plan selected before context loading.
    #[must_use]
    pub fn execution_plan(&self) -> Option<&Arc<ExecutionPlan>> {
        self.script().execution_plan()
    }

    /// Returns the reference counter associated with this context.
    #[must_use]
    pub fn reference_counter(&self) -> &ReferenceCounter {
        self.shared_states.reference_counter()
    }

    /// Returns the script hash for this context as a 20-byte array.
    /// This mirrors the C# `Script.ToScriptHash()` behaviour (Hash160).
    #[inline]
    #[must_use]
    pub fn script_hash(&self) -> [u8; 20] {
        self.script().script_hash()
    }

    /// Returns the current instruction pointer.
    #[inline]
    #[must_use]
    pub const fn instruction_pointer(&self) -> usize {
        self.instruction_pointer
    }

    /// Sets the instruction pointer.
    ///
    /// Neo.VM permits the position immediately after the script so the engine
    /// can execute its synthetic `RET`, but rejects positions beyond it.
    #[inline]
    pub fn set_instruction_pointer(&mut self, position: usize) -> VmResult<()> {
        if position > self.script().len() {
            return Err(VmError::invalid_operation_msg(format!(
                "Instruction pointer is out of script bounds: {position}/{}",
                self.script().len()
            )));
        }
        self.instruction_pointer = position;
        Ok(())
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

    /// Attaches the shared evaluation stack to an explicitly enabled profiler.
    pub(crate) fn set_stack_profile(&self, profile: StackProfileHandle) {
        self.shared_states
            .evaluation_stack_mut()
            .set_profile(profile);
    }

    /// Returns true when static fields are initialized for this context.
    #[must_use]
    pub fn has_static_fields(&self) -> bool {
        self.shared_states.has_static_fields()
    }

    /// Executes a closure with mutable access to static fields.
    pub fn with_static_fields_mut<R, F: FnOnce(&mut Option<Slot>) -> R>(&self, f: F) -> R {
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

    /// Returns true when both contexts share the same typed state.
    #[must_use]
    pub fn shares_state_with(&self, other: &Self) -> bool {
        self.shared_states.state_ptr_eq(&other.shared_states)
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

    /// Returns the shared typed state for this execution context.
    #[must_use]
    pub fn state(&self) -> Arc<Mutex<S>> {
        self.shared_states.state()
    }

    /// Executes a closure with mutable access to the shared typed state.
    pub fn with_state<R, F: FnOnce(&mut S) -> R>(&self, f: F) -> R {
        self.shared_states.with_state(f)
    }

    /// Replaces the shared typed state value and returns the previous state.
    pub fn replace_state(&self, state: S) -> S {
        self.shared_states.replace_state(state)
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
    ) -> VmResult<Self>
    where
        S: Default,
    {
        // Create a new shared states with the new reference counter
        let shared_states = SharedStates::new(self.script().clone(), reference_counter.clone());

        // Copy the evaluation stack
        {
            let source_stack = self.evaluation_stack();
            let mut target_stack = shared_states.evaluation_stack_mut();
            source_stack.copy_to(&mut target_stack, None)?;
        }

        // Create the new context

        // Note: Not cloning static_fields, local_variables, or arguments
        // as they would need separate reference counter handling

        Ok(Self {
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
    pub fn clone_with_position(&self, position: usize) -> VmResult<Self> {
        let mut context = Self {
            shared_states: self.shared_states.clone(),
            instruction_pointer: 0,
            rvcount: 0,
            local_variables: None,
            arguments: None,
            try_stack: None,
        };
        context.set_instruction_pointer(position)?;
        Ok(context)
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
}

impl<S> Clone for ExecutionContext<S> {
    fn clone(&self) -> Self {
        Self {
            shared_states: self.shared_states.clone(),
            instruction_pointer: self.instruction_pointer,
            rvcount: 0,
            local_variables: None,
            arguments: None,
            try_stack: None,
        }
    }
}

#[cfg(test)]
#[path = "../tests/execution_context/context.rs"]
mod tests;
