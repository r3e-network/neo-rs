//
// stack.rs - Stack operations (peek, pop, push) and gas tracking
//

use super::{ExecutionEngine, StackItem, VmError, VmResult, DEFAULT_GAS_LIMIT};

impl ExecutionEngine {
    /// Returns the item at the specified index from the top of the current stack without removing it.
    pub fn peek(&self, index: usize) -> VmResult<StackItem> {
        let context = self
            .current_context()
            .ok_or_else(|| VmError::invalid_operation_msg("No current context"))?;
        context.evaluation_stack().peek(index).cloned()
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

    /// Adds gas consumed and checks if the limit has been exceeded.
    /// Returns `VmError::GasExhausted` if the gas limit is exceeded.
    ///
    /// # Arguments
    /// * `gas` - Amount of gas to add (can be negative for refunds, though refunds are clamped to 0)
    pub fn add_gas_consumed(&mut self, gas: i64) -> VmResult<()> {
        // Handle negative gas (refunds) - clamp to not go below 0
        if gas < 0 {
            let abs_gas = gas.unsigned_abs();
            self.gas_consumed = self.gas_consumed.saturating_sub(abs_gas);
        } else {
            let gas_u64 = gas as u64;
            // Check if adding this gas would exceed the limit
            if self.gas_consumed.saturating_add(gas_u64) > self.gas_limit {
                return Err(VmError::gas_exhausted(self.gas_consumed, self.gas_limit));
            }
            self.gas_consumed = self.gas_consumed.saturating_add(gas_u64);
        }
        Ok(())
    }

    /// Returns the total gas consumed so far.
    #[inline]
    #[must_use]
    pub const fn gas_consumed(&self) -> u64 {
        self.gas_consumed
    }

    /// Returns the gas limit for this execution.
    #[inline]
    #[must_use]
    pub const fn gas_limit(&self) -> u64 {
        self.gas_limit
    }

    /// Sets the gas limit for this execution.
    ///
    /// # Note
    /// This should typically be set before execution begins. Changing the limit
    /// during execution may cause unexpected behavior.
    pub fn set_gas_limit(&mut self, limit: u64) {
        self.gas_limit = limit;
    }

    /// Returns the remaining gas available for execution.
    #[inline]
    #[must_use]
    pub const fn gas_remaining(&self) -> u64 {
        self.gas_limit.saturating_sub(self.gas_consumed)
    }

    /// Checks if the gas limit has been reached or exceeded.
    #[inline]
    #[must_use]
    pub const fn is_gas_exhausted(&self) -> bool {
        self.gas_consumed >= self.gas_limit
    }

    /// Resets the gas consumed to 0.
    ///
    /// # Note
    /// This should typically only be used when resetting the engine for a new execution.
    pub fn reset_gas_consumed(&mut self) {
        self.gas_consumed = 0;
    }

    /// Returns the default gas limit constant.
    #[inline]
    #[must_use]
    pub const fn default_gas_limit() -> u64 {
        DEFAULT_GAS_LIMIT
    }
}
