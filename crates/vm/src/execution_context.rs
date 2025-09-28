//! Execution context module for the Neo Virtual Machine.
//!
//! This module provides the execution context implementation for the Neo VM.

use crate::error::VmError;
use crate::error::VmResult;
use crate::evaluation_stack::EvaluationStack;
use crate::exception_handling_context::ExceptionHandlingContext;
use crate::instruction::Instruction;
use crate::reference_counter::ReferenceCounter;
use crate::script::Script;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

/// Maximum exception handler stack size
/// A slot for storing variables or arguments in a context.
///
/// This is a thin alias over [`crate::slot::Slot`] so existing call sites
/// can continue to reference `execution_context::Slot`.
pub type Slot = crate::slot::Slot;

/// Shared states for execution contexts that can be cloned and shared.
/// This matches the C# implementation's SharedStates class exactly.
#[derive(Clone)]
pub struct SharedStates {
    script: Arc<Script>,
    pub evaluation_stack: crate::evaluation_stack::EvaluationStack,
    pub static_fields: Option<Slot>,
    pub reference_counter: ReferenceCounter,
    /// State map matching C# Dictionary<Type, object>
    states: Arc<RwLock<HashMap<TypeId, Box<dyn Any + Send + Sync>>>>,
}

impl std::fmt::Debug for SharedStates {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedStates")
            .field("script", &"<Script>")
            .field("evaluation_stack", &"<EvaluationStack>")
            .field("static_fields", &self.static_fields)
            .finish()
    }
}

impl SharedStates {
    pub fn new(script: crate::script::Script, reference_counter: ReferenceCounter) -> Self {
        let script = Arc::new(script);
        Self {
            script: Arc::clone(&script),
            evaluation_stack: crate::evaluation_stack::EvaluationStack::new(
                reference_counter.clone(),
            ),
            static_fields: None,
            reference_counter,
            states: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn reference_counter(&self) -> &ReferenceCounter {
        &self.reference_counter
    }

    pub fn script(&self) -> &crate::script::Script {
        self.script.as_ref()
    }

    pub fn script_arc(&self) -> Arc<crate::script::Script> {
        Arc::clone(&self.script)
    }

    pub fn evaluation_stack(&self) -> &crate::evaluation_stack::EvaluationStack {
        &self.evaluation_stack
    }

    pub fn evaluation_stack_mut(&mut self) -> &mut crate::evaluation_stack::EvaluationStack {
        &mut self.evaluation_stack
    }

    pub fn static_fields(&self) -> Option<&Slot> {
        self.static_fields.as_ref()
    }

    pub fn static_fields_mut(&mut self) -> &mut Option<Slot> {
        &mut self.static_fields
    }

    pub fn set_static_fields(&mut self, static_fields: Option<Slot>) {
        self.static_fields = static_fields;
    }

    /// Gets custom data of the specified type. If the data does not exist, create a new one.
    /// Mirrors the C# SharedStates.GetState<T>() behaviour by caching instances per type.
    pub fn get_state<T: 'static + Default + Send + Sync>(&self) -> Arc<Mutex<T>> {
        self.get_state_with_factory(T::default)
    }

    /// Gets custom data of the specified type, creating it with the provided factory if it doesn't exist.
    /// Mirrors the C# SharedStates.GetState<T>(Func<T>) API.
    pub fn get_state_with_factory<T: 'static + Send + Sync, F: FnOnce() -> T>(
        &self,
        factory: F,
    ) -> Arc<Mutex<T>> {
        let type_id = TypeId::of::<T>();

        if let Some(existing) = self.states.read().expect("Lock poisoned").get(&type_id) {
            if let Some(arc) = existing.downcast_ref::<Arc<Mutex<T>>>() {
                return Arc::clone(arc);
            }
        }

        let new_state = Arc::new(Mutex::new(factory()));
        let mut states = self.states.write().expect("Lock poisoned");
        states.insert(type_id, Box::new(Arc::clone(&new_state)));
        new_state
    }

    /// Sets a state value by type.
    /// This matches the C# implementation's state setting behavior.
    pub fn set_state<T: 'static + Send + Sync>(&self, value: T) {
        let type_id = TypeId::of::<T>();
        let state = Arc::new(Mutex::new(value));

        let mut states = self.states.write().expect("Lock poisoned");
        states.insert(type_id, Box::new(state));
    }
}

/// Represents an execution context in the Neo Virtual Machine.
/// This matches the C# implementation's ExecutionContext class.
#[derive(Clone)]
pub struct ExecutionContext {
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
    pub fn new(script: Script, rvcount: i32, reference_counter: &ReferenceCounter) -> Self {
        Self {
            shared_states: SharedStates::new(script, reference_counter.clone()),
            instruction_pointer: 0,
            rvcount,
            local_variables: None,
            arguments: None,
            try_stack: None,
        }
    }

    /// Returns the script for this context.
    /// This matches the C# implementation's Script property.
    pub fn script(&self) -> &Script {
        self.shared_states.script()
    }

    /// Returns the script as an Arc for identity-sensitive operations (matches C# reference semantics).
    pub fn script_arc(&self) -> Arc<Script> {
        self.shared_states.script_arc()
    }

    /// Returns the script hash for this context as a 20-byte array.
    /// This mirrors the C# `Script.ToScriptHash()` behaviour (Hash160).
    pub fn script_hash(&self) -> [u8; 20] {
        #[allow(unused_imports)]
        use ripemd::{Digest as _, Ripemd160};
        #[allow(unused_imports)]
        use sha2::{Digest as _, Sha256};

        let script_bytes = self.script().as_bytes();

        // Calculate SHA-256 hash
        let mut sha256_hasher = Sha256::new();
        sha256_hasher.update(script_bytes);
        let sha256_hash = sha256_hasher.finalize();

        // Calculate RIPEMD-160 hash of the SHA-256 hash
        let mut ripemd_hasher = Ripemd160::new();
        ripemd_hasher.update(sha256_hash);
        let ripemd_hash = ripemd_hasher.finalize();

        let mut result = [0u8; 20];
        result.copy_from_slice(&ripemd_hash);
        result
    }

    /// Returns the current instruction pointer.
    pub fn instruction_pointer(&self) -> usize {
        self.instruction_pointer
    }

    /// Sets the instruction pointer.
    pub fn set_instruction_pointer(&mut self, position: usize) {
        self.instruction_pointer = position;
    }

    /// Returns the current instruction or None if at the end of the script.
    /// This matches the C# implementation's CurrentInstruction property.
    pub fn current_instruction(&self) -> VmResult<Instruction> {
        if self.instruction_pointer >= self.script().len() {
            return Err(VmError::invalid_operation_msg(
                "Instruction pointer is out of range",
            ));
        }

        self.script().get_instruction(self.instruction_pointer)
    }

    /// Returns the next instruction or None if at the end of the script.
    /// This matches the C# implementation's NextInstruction property.
    pub fn next_instruction(&self) -> VmResult<Instruction> {
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
    pub fn rvcount(&self) -> i32 {
        self.rvcount
    }

    /// Returns the evaluation stack for this context.
    /// This matches the C# implementation's EvaluationStack property.
    pub fn evaluation_stack(&self) -> &EvaluationStack {
        self.shared_states.evaluation_stack()
    }

    /// Returns the evaluation stack for this context (mutable).
    /// This matches the C# implementation's EvaluationStack property.
    pub fn evaluation_stack_mut(&mut self) -> &mut EvaluationStack {
        self.shared_states.evaluation_stack_mut()
    }

    /// Returns the static fields for this context.
    /// This matches the C# implementation's StaticFields property getter.
    pub fn static_fields(&self) -> Option<&Slot> {
        self.shared_states.static_fields()
    }

    /// Returns the static fields for this context (mutable).
    /// This matches the C# implementation's StaticFields property setter.
    pub fn static_fields_mut(&mut self) -> &mut Option<Slot> {
        self.shared_states.static_fields_mut()
    }

    /// Sets the static fields for this context.
    pub fn set_static_fields(&mut self, static_fields: Option<Slot>) {
        self.shared_states.set_static_fields(static_fields);
    }

    /// Returns the local variables for this context.
    pub fn local_variables(&self) -> Option<&Slot> {
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
    pub fn arguments(&self) -> Option<&Slot> {
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
    pub fn try_stack(&self) -> Option<&Vec<ExceptionHandlingContext>> {
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
    pub fn try_stack_len(&self) -> usize {
        self.try_stack
            .as_ref()
            .map(|stack| stack.len())
            .unwrap_or(0)
    }

    /// Returns true when at least one try context is active.
    pub fn has_try_context(&self) -> bool {
        self.try_stack_len() > 0
    }

    /// Pushes a new try context onto the stack, initialising the stack if needed.
    pub fn push_try_context(&mut self, ctx: ExceptionHandlingContext) {
        self.try_stack.get_or_insert_with(Vec::new).push(ctx);
    }

    /// Pops the current try context if one exists.
    pub fn pop_try_context(&mut self) -> Option<ExceptionHandlingContext> {
        self.try_stack.as_mut().and_then(|stack| stack.pop())
    }

    /// Returns the current try context if one exists.
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

    /// Gets a state value for the specified type, creating it if it doesn't exist.
    /// Mirrors the C# ExecutionContext.GetState<T>() helper.
    pub fn get_state<T: 'static + Default + Send + Sync>(&self) -> Arc<Mutex<T>> {
        self.shared_states.get_state::<T>()
    }

    /// Gets a state value for the specified type using the provided factory when absent.
    /// Mirrors the C# ExecutionContext.GetState<T>(Func<T>) helper.
    pub fn get_state_with_factory<T: 'static + Send + Sync, F: FnOnce() -> T>(
        &self,
        factory: F,
    ) -> Arc<Mutex<T>> {
        self.shared_states.get_state_with_factory(factory)
    }

    /// Sets a state value by key.
    /// This matches the C# implementation's state setting behavior.
    pub fn set_state<T: 'static + Send + Sync>(&self, value: T) {
        self.shared_states.set_state(value);
    }

    /// Push an item onto the evaluation stack
    pub fn push(&mut self, item: crate::stack_item::StackItem) -> VmResult<()> {
        self.shared_states.evaluation_stack_mut().push(item);
        Ok(())
    }

    /// Pop an item from the evaluation stack
    pub fn pop(&mut self) -> VmResult<crate::stack_item::StackItem> {
        self.shared_states.evaluation_stack_mut().pop()
    }

    /// Peek at an item on the evaluation stack without removing it
    pub fn peek(&self, index: usize) -> VmResult<crate::stack_item::StackItem> {
        self.shared_states
            .evaluation_stack()
            .peek(index)
            .map(|item| item.clone())
    }

    /// Clones the context with a new reference counter.
    pub fn clone_for_reference_counter(&self, reference_counter: &ReferenceCounter) -> Self {
        // Create a new shared states with the new reference counter
        let mut shared_states = SharedStates::new(self.script().clone(), reference_counter.clone());

        // Copy the evaluation stack
        self
            .evaluation_stack()
            .copy_to(shared_states.evaluation_stack_mut(), None)
            .expect("evaluation stack copy should succeed");

        // Create the new context

        // Note: Not cloning static_fields, local_variables, or arguments
        // as they would need separate reference counter handling

        Self {
            shared_states,
            instruction_pointer: self.instruction_pointer,
            rvcount: self.rvcount,
            local_variables: None,
            arguments: None,
            try_stack: self.try_stack.clone(),
        }
    }

    /// Clones the context so that they share the same script, stack, and static fields.
    /// This matches the C# implementation's Clone method.
    pub fn clone(&self) -> Self {
        self.clone_with_position(self.instruction_pointer)
    }

    /// Clones the context so that they share the same script, stack, and static fields.
    /// This matches the C# implementation's Clone(int) method.
    pub fn clone_with_position(&self, position: usize) -> Self {
        Self {
            shared_states: self.shared_states.clone(),
            instruction_pointer: position,
            rvcount: 0, // The C# implementation sets this to 0
            local_variables: None,
            arguments: None,
            try_stack: None,
        }
    }

    /// Gets a shared state by type, creating it if it doesn't exist.
    /// Mirrors the C# ExecutionContext.GetState<T>() helper for shared state access.
    pub fn get_shared_state<T: 'static + Default + Send + Sync>(&self) -> Arc<Mutex<T>> {
        self.shared_states.get_state::<T>()
    }

    /// Gets a shared state by type, creating it with the provided factory if it doesn't exist.
    pub fn get_shared_state_with_factory<T: 'static + Send + Sync, F: FnOnce() -> T>(
        &self,
        factory: F,
    ) -> Arc<Mutex<T>> {
        self.shared_states.get_state_with_factory(factory)
    }

    /// Initializes the slots for local variables and arguments.
    pub fn init_slot(&mut self, local_count: usize, argument_count: usize) -> VmResult<()> {
        // Initialize local variables
        if local_count > 0 {
            let reference_counter = self.shared_states.reference_counter().clone();
            self.local_variables = Some(Slot::new(local_count, reference_counter));
        }

        // Initialize arguments
        if argument_count > 0 {
            let reference_counter = self.shared_states.reference_counter().clone();
            self.arguments = Some(Slot::new(argument_count, reference_counter));
        }

        Ok(())
    }

    /// Loads a value from a static field.
    pub fn load_static_field(&self, index: usize) -> VmResult<crate::stack_item::StackItem> {
        if let Some(static_fields) = self.shared_states.static_fields() {
            static_fields
                .get(index)
                .cloned()
                .ok_or_else(|| VmError::invalid_operation_msg("Static field index out of range"))
        } else {
            Err(VmError::invalid_operation_msg(
                "No static fields initialized",
            ))
        }
    }

    /// Stores a value to a static field.
    pub fn store_static_field(
        &mut self,
        index: usize,
        value: crate::stack_item::StackItem,
    ) -> VmResult<()> {
        if let Some(static_fields) = self.shared_states.static_fields_mut() {
            static_fields.set(index, value)?;
            Ok(())
        } else {
            Err(VmError::invalid_operation_msg(
                "No static fields initialized",
            ))
        }
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

#[cfg(test)]
#[allow(dead_code)]
mod tests {
    use super::*;
    use crate::op_code::OpCode;
    use crate::stack_item::StackItem;
    use num_bigint::BigInt;
    use std::sync::Mutex;

    #[derive(Default)]
    struct TestFlag {
        flag: bool,
    }

    #[test]
    fn test_execution_context_creation() {
        let script_bytes = vec![OpCode::PUSH1 as u8, OpCode::PUSH2 as u8, OpCode::ADD as u8];
        let script = Script::new_relaxed(script_bytes);
        let reference_counter = ReferenceCounter::new();

        let context = ExecutionContext::new(script, -1, &reference_counter);

        assert_eq!(context.instruction_pointer(), 0);
        assert_eq!(context.rvcount(), -1);
        assert_eq!(
            context
                .current_instruction()
                .expect("intermediate value should exist")
                .opcode(),
            OpCode::PUSH1
        );
        assert!(context.evaluation_stack().is_empty());
        assert!(context.try_stack().is_none());
    }

    #[test]
    fn test_move_next() {
        let script_bytes = vec![OpCode::PUSH1 as u8, OpCode::PUSH2 as u8, OpCode::ADD as u8];
        let script = Script::new_relaxed(script_bytes);
        let reference_counter = ReferenceCounter::new();

        let mut context = ExecutionContext::new(script, -1, &reference_counter);

        assert_eq!(
            context
                .current_instruction()
                .expect("intermediate value should exist")
                .opcode(),
            OpCode::PUSH1
        );

        context.move_next().expect("VM operation should succeed");
        assert_eq!(context.instruction_pointer(), 1);
        assert_eq!(
            context
                .current_instruction()
                .expect("intermediate value should exist")
                .opcode(),
            OpCode::PUSH2
        );

        context.move_next().expect("VM operation should succeed");
        assert_eq!(context.instruction_pointer(), 2);
        assert_eq!(
            context
                .current_instruction()
                .expect("intermediate value should exist")
                .opcode(),
            OpCode::ADD
        );

        context.move_next().expect("VM operation should succeed");
        assert_eq!(context.instruction_pointer(), 3);
        assert!(context.current_instruction().is_err());
    }

    #[test]
    fn test_try_stack() {
        let script_bytes = vec![OpCode::NOP as u8];
        let script = Script::new_relaxed(script_bytes);
        let reference_counter = ReferenceCounter::new();

        let mut context = ExecutionContext::new(script, -1, &reference_counter);

        // Initially, try_stack is None
        assert!(context.try_stack().is_none());

        // Create a try stack with one context
        use crate::exception_handling_context::ExceptionHandlingContext;
        use crate::exception_handling_state::ExceptionHandlingState;
        let mut try_stack = Vec::new();
        let try_context = ExceptionHandlingContext::new(10, 20);
        try_stack.push(try_context);

        // Set the try stack
        context.set_try_stack(Some(try_stack));

        // Check that the try stack is set
        assert!(context.try_stack().is_some());
        assert_eq!(
            context
                .try_stack()
                .expect("intermediate value should exist")
                .len(),
            1
        );
        assert_eq!(
            context.try_stack().expect("VM operation should succeed")[0].catch_pointer(),
            10
        );
        assert_eq!(
            context.try_stack().expect("Operation failed")[0].finally_pointer(),
            20
        );

        // Modify the try stack
        if let Some(stack) = context.try_stack_mut() {
            let exception_context = &mut stack[0];
            exception_context.set_state(ExceptionHandlingState::Catch);
        }

        // Check that the modification was applied
        assert_eq!(
            context.try_stack().expect("Operation failed")[0].state(),
            ExceptionHandlingState::Catch
        );
    }

    #[test]
    fn test_slot() {
        let reference_counter = ReferenceCounter::new();

        let items = vec![
            StackItem::from_int(1),
            StackItem::from_int(2),
            StackItem::from_int(3),
        ];

        let mut slot = Slot::with_items(items, reference_counter.clone());

        assert_eq!(slot.len(), 3);
        assert_eq!(
            slot.get(0)
                .expect("Index out of bounds")
                .as_int()
                .expect("VM operation should succeed"),
            BigInt::from(1)
        );
        assert_eq!(
            slot.get(1)
                .expect("Index out of bounds")
                .as_int()
                .expect("VM operation should succeed"),
            BigInt::from(2)
        );
        assert_eq!(
            slot.get(2)
                .expect("Index out of bounds")
                .as_int()
                .expect("VM operation should succeed"),
            BigInt::from(3)
        );

        slot.set(1, StackItem::from_int(42)).unwrap();
        assert_eq!(
            slot.get(1)
                .expect("Index out of bounds")
                .as_int()
                .expect("VM operation should succeed"),
            BigInt::from(42)
        );

        assert!(slot.set(5, StackItem::from_int(5)).is_err());

        slot.clear_references();
        assert_eq!(slot.len(), 3);
        assert!(slot.iter().all(|item| item.is_null()));
    }

    #[test]
    fn test_next_instruction() {
        let script_bytes = vec![
            OpCode::PUSH1 as u8,
            OpCode::PUSH2 as u8,
            OpCode::ADD as u8,
            OpCode::RET as u8,
        ];

        let script = Script::new_relaxed(script_bytes);
        let reference_counter = ReferenceCounter::new();

        let context = ExecutionContext::new(script, -1, &reference_counter);

        // Test current instruction
        let current = context
            .current_instruction()
            .expect("VM operation should succeed");
        assert_eq!(current.opcode(), OpCode::PUSH1);

        // Test next instruction
        let next = context
            .next_instruction()
            .expect("VM operation should succeed");
        assert_eq!(next.opcode(), OpCode::PUSH2);
    }

    #[test]
    fn test_clone() {
        let script_bytes = vec![
            OpCode::PUSH1 as u8,
            OpCode::PUSH2 as u8,
            OpCode::ADD as u8,
            OpCode::RET as u8,
        ];

        let script = Script::new_relaxed(script_bytes);
        let reference_counter = ReferenceCounter::new();

        let mut context = ExecutionContext::new(script, -1, &reference_counter);

        // Push a value onto the stack
        context.evaluation_stack_mut().push(StackItem::from_int(42));

        // Clone the context
        let clone = context.clone();

        // Check that the clone has the same script and stack
        assert_eq!(clone.script().to_array(), context.script().to_array());
        assert_eq!(
            clone
                .evaluation_stack()
                .peek(0)
                .expect("intermediate value should exist")
                .as_int()
                .expect("VM operation should succeed"),
            BigInt::from(42)
        );

        // Check that the clone has a different instruction pointer
        assert_eq!(clone.instruction_pointer(), context.instruction_pointer());

        // Check that the clone has a different rvcount
        assert_eq!(clone.rvcount(), 0);

        // Clone with a different position
        let clone_with_position = context.clone_with_position(2);

        // Check that the clone has a different instruction pointer
        assert_eq!(clone_with_position.instruction_pointer(), 2);
    }

    #[test]
    fn test_get_state() -> Result<(), Box<dyn std::error::Error>> {
        let script_bytes = vec![OpCode::NOP as u8];
        let script = Script::new_relaxed(script_bytes);
        let reference_counter = ReferenceCounter::new();

        let context = ExecutionContext::new(script, -1, &reference_counter);

        let flag_state = context.get_state_with_factory::<TestFlag, _>(|| TestFlag { flag: true });
        {
            let mut flag = flag_state.lock().map_err(|_| "poisoned")?;
            assert!(flag.flag);
            flag.flag = false;
        }

        let flag_state_again = context.get_state::<TestFlag>();
        {
            let flag = flag_state_again.lock().map_err(|_| "poisoned")?;
            assert!(!flag.flag);
        }

        let stack_state = context.get_state_with_factory::<Vec<i32>, _>(|| Vec::new());
        {
            let mut stack = stack_state.lock().map_err(|_| "poisoned")?;
            stack.push(100);
        }

        let clone = context.clone();
        let cloned_stack_state = clone.get_state::<Vec<i32>>();
        {
            let mut stack = cloned_stack_state.lock().map_err(|_| "poisoned")?;
            assert_eq!(stack.pop(), Some(100));
            stack.push(200);
        }

        let original_stack_state = context.get_state::<Vec<i32>>();
        {
            let mut stack = original_stack_state.lock().map_err(|_| "poisoned")?;
            assert_eq!(stack.pop(), Some(200));
        }
        Ok(())
    }

    #[test]
    fn test_get_shared_state() -> Result<(), Box<dyn std::error::Error>> {
        let script_bytes = vec![OpCode::NOP as u8];
        let script = Script::new_relaxed(script_bytes);
        let reference_counter = ReferenceCounter::new();

        let context = ExecutionContext::new(script, -1, &reference_counter);

        let shared_vec = context.get_shared_state::<Vec<i32>>();
        {
            let mut vec = shared_vec.lock().map_err(|_| "poisoned")?;
            vec.push(100);
        }

        let shared_vec_again = context.get_shared_state::<Vec<i32>>();
        {
            let vec = shared_vec_again.lock().map_err(|_| "poisoned")?;
            assert_eq!(*vec, vec![100]);
        }

        let shared_with_factory =
            context.get_shared_state_with_factory::<Vec<i32>, _>(|| vec![200]);
        {
            let vec = shared_with_factory.lock().map_err(|_| "poisoned")?;
            assert_eq!(*vec, vec![100]);
        }

        let clone = context.clone();
        let clone_shared_vec = clone.get_shared_state::<Vec<i32>>();
        {
            let mut vec = clone_shared_vec.lock().map_err(|_| "poisoned")?;
            vec.push(300);
        }

        let context_shared_vec = context.get_shared_state::<Vec<i32>>();
        {
            let vec = context_shared_vec.lock().map_err(|_| "poisoned")?;
            assert_eq!(*vec, vec![100, 300]);
        }

        context.set_state(vec![1, 2, 3]);
        let context_shared_vec_after = context.get_shared_state::<Vec<i32>>();

        {
            let vec = context_shared_vec_after.lock().map_err(|_| "poisoned")?;
            assert_eq!(*vec, vec![1, 2, 3]);
        }

        Ok(())
    }
}
