//
// stack.rs - Stack operations (peek, pop, push)
//

use super::*;

impl ExecutionEngine {
    /// Returns the item at the specified index from the top of the current stack without removing it.
    pub fn peek(&self, index: usize) -> VmResult<&StackItem> {
        let context = self
            .current_context()
            .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
        context.evaluation_stack().peek(index)
    }

    /// Removes and returns the item at the top of the current stack.
    pub fn pop(&mut self) -> VmResult<StackItem> {
        let context = self
            .current_context_mut()
            .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
        context.evaluation_stack_mut().pop()
    }

    /// Pushes an item onto the top of the current stack.
    pub fn push(&mut self, item: StackItem) -> VmResult<()> {
        let context = self
            .current_context_mut()
            .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
        context.push(item)
    }

    /// Adds gas consumed (integrated with gas calculator)
    /// ApplicationEngine overrides this with additional gas tracking
    pub fn add_gas_consumed(&mut self, _gas: i64) -> VmResult<()> {
        // Gas tracking disabled - no C# counterpart
        Ok(())
    }
}
