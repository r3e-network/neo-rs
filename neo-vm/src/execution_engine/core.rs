//
// core.rs - ExecutionEngine constructor, state management, and basic getters/setters
//

use super::{
    CallFlags, EvaluationStack, ExecutionContext, ExecutionEngine, ExecutionEngineLimits,
    InteropService, JumpTable, ReferenceCounter, StackItem, VMState, VmError, DEFAULT_GAS_LIMIT,
};

impl ExecutionEngine {
    /// Creates a new execution engine with the specified jump table.
    #[must_use]
    pub fn new(jump_table: Option<JumpTable>) -> Self {
        let reference_counter = ReferenceCounter::new();
        Self::new_with_limits(
            jump_table,
            reference_counter,
            ExecutionEngineLimits::default(),
        )
    }

    /// Creates a new execution engine with the specified reference counter and limits.
    pub fn new_with_limits(
        jump_table: Option<JumpTable>,
        reference_counter: ReferenceCounter,
        limits: ExecutionEngineLimits,
    ) -> Self {
        Self {
            state: VMState::BREAK,
            is_jumping: false,
            jump_table: jump_table.unwrap_or_else(JumpTable::default),
            limits,
            reference_counter: reference_counter.clone(),
            interop_service: Some(InteropService::new()),
            interop_host: None,
            call_flags: CallFlags::ALL,
            invocation_stack: Vec::new(),
            result_stack: EvaluationStack::new(reference_counter),
            uncaught_exception: None,
            instructions_executed: 0,
            gas_consumed: 0,
            gas_limit: DEFAULT_GAS_LIMIT,
        }
    }

    /// Returns the current state of the VM.
    #[inline]
    #[must_use]
    pub const fn state(&self) -> VMState {
        self.state
    }

    /// Sets the state of the VM.
    #[inline]
    pub fn set_state(&mut self, state: VMState) {
        if self.state != state {
            self.state = state;
            self.on_state_changed();
        }
    }

    /// Called when the VM state changes.
    #[inline]
    fn on_state_changed(&mut self) {}

    /// Called when an exception causes the VM to enter the FAULT state.
    pub(crate) fn on_fault(&mut self, err: VmError) {
        #[cfg(debug_assertions)]
        println!("ExecutionEngine fault: {err:?}");
        if self.uncaught_exception.is_none() {
            let message = match &err {
                VmError::CatchableException { message } => message.clone(),
                _ => {
                    let mut fault_text = err.to_string();
                    if let Some(context) = self.current_context() {
                        let ip = context.instruction_pointer();
                        let opcode = context
                            .current_instruction()
                            .map(|instruction| format!("{:?}", instruction.opcode()))
                            .unwrap_or_else(|_| "<none>".to_string());
                        let eval_depth = context.evaluation_stack().len();
                        fault_text = format!(
                            "{fault_text} [ip={ip} opcode={opcode} eval_depth={eval_depth}]"
                        );
                    }
                    fault_text
                }
            };
            self.uncaught_exception = Some(StackItem::from_byte_string(message.into_bytes()));
        }
        self.set_state(VMState::FAULT);
    }

    /// Returns the reference counter.
    #[inline]
    #[must_use]
    pub const fn reference_counter(&self) -> &ReferenceCounter {
        &self.reference_counter
    }

    /// Returns the execution limits configured for this engine.
    #[inline]
    #[must_use]
    pub const fn limits(&self) -> &ExecutionEngineLimits {
        &self.limits
    }

    /// Returns the invocation stack.
    #[inline]
    #[must_use]
    pub fn invocation_stack(&self) -> &[ExecutionContext] {
        &self.invocation_stack
    }

    /// Returns a mutable handle to the invocation stack.
    #[inline]
    pub(crate) fn invocation_stack_mut(&mut self) -> &mut Vec<ExecutionContext> {
        &mut self.invocation_stack
    }

    /// Returns the current context, if any.
    #[inline]
    #[must_use]
    pub fn current_context(&self) -> Option<&ExecutionContext> {
        self.invocation_stack.last()
    }

    /// Returns the current context (mutable), if any.
    #[inline]
    pub fn current_context_mut(&mut self) -> Option<&mut ExecutionContext> {
        self.invocation_stack.last_mut()
    }

    /// Returns the entry context, if any.
    #[inline]
    #[must_use]
    pub fn entry_context(&self) -> Option<&ExecutionContext> {
        self.invocation_stack.first()
    }

    /// Returns the result stack.
    #[inline]
    #[must_use]
    pub const fn result_stack(&self) -> &EvaluationStack {
        &self.result_stack
    }

    /// Returns the result stack (mutable).
    #[inline]
    pub fn result_stack_mut(&mut self) -> &mut EvaluationStack {
        &mut self.result_stack
    }

    /// Returns the uncaught exception, if any.
    #[inline]
    #[must_use]
    pub const fn uncaught_exception(&self) -> Option<&StackItem> {
        self.uncaught_exception.as_ref()
    }

    /// Sets the uncaught exception.
    #[inline]
    pub fn set_uncaught_exception(&mut self, exception: Option<StackItem>) {
        self.uncaught_exception = exception;
    }

    /// Gets the uncaught exception (matches C# `UncaughtException` property exactly).
    #[inline]
    #[must_use]
    pub const fn get_uncaught_exception(&self) -> Option<&StackItem> {
        self.uncaught_exception.as_ref()
    }

    /// Handles an exception by setting it as uncaught and transitioning to FAULT state.
    /// Returns true if the exception was handled, false otherwise.
    /// This matches C# exception handling behavior exactly.
    pub fn handle_exception(&mut self) -> bool {
        if self.uncaught_exception.is_some() {
            self.set_state(VMState::FAULT);
            true
        } else {
            false
        }
    }

    /// Returns the effective call flags for this engine.
    #[must_use]
    pub const fn call_flags(&self) -> CallFlags {
        self.call_flags
    }

    /// Sets the effective call flags for this engine.
    pub fn set_call_flags(&mut self, flags: CallFlags) {
        self.call_flags = flags;
    }

    /// Checks whether the required call flags are satisfied.
    #[must_use]
    pub const fn has_call_flags(&self, required: CallFlags) -> bool {
        required.is_empty() || self.call_flags.contains(required)
    }

    /// Returns the jump table.
    #[must_use]
    pub const fn jump_table(&self) -> &JumpTable {
        &self.jump_table
    }

    /// Returns the jump table (mutable).
    pub fn jump_table_mut(&mut self) -> &mut JumpTable {
        &mut self.jump_table
    }

    /// Sets the jump table.
    pub fn set_jump_table(&mut self, jump_table: JumpTable) {
        self.jump_table = jump_table;
    }
}
