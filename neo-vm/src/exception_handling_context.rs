//! Exception handling context for the Neo Virtual Machine.
//!
//! Mirrors `Neo.VM/ExceptionHandlingContext.cs` so that the execution engine can
//! manage nested try/catch/finally regions with the same semantics as the C#
//! reference implementation.

use crate::exception_handling_state::ExceptionHandlingState;

/// Represents the context pushed on the invocation stack when a `try`
/// instruction is executed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExceptionHandlingContext {
    catch_pointer: i32,
    finally_pointer: i32,
    end_pointer: i32,
    state: ExceptionHandlingState,
}

impl ExceptionHandlingContext {
    /// Creates a new exception handling context.
    ///
    /// * `catch_pointer` – the relative position of the associated `catch`
    ///   block, or `-1` when no catch block exists.
    /// * `finally_pointer` – the relative position of the associated `finally`
    ///   block, or `-1` when no finally block exists.
    #[must_use] 
    pub const fn new(catch_pointer: i32, finally_pointer: i32) -> Self {
        Self {
            catch_pointer,
            finally_pointer,
            end_pointer: -1,
            state: ExceptionHandlingState::Try,
        }
    }

    /// Returns the position of the `catch` block. `-1` means no catch block.
    #[must_use] 
    pub const fn catch_pointer(&self) -> i32 {
        self.catch_pointer
    }

    /// Returns the position of the `finally` block. `-1` means no finally block.
    #[must_use] 
    pub const fn finally_pointer(&self) -> i32 {
        self.finally_pointer
    }

    /// Returns the position to jump to once the current handler is finished.
    #[must_use] 
    pub const fn end_pointer(&self) -> i32 {
        self.end_pointer
    }

    /// Updates the end pointer for this context.
    pub fn set_end_pointer(&mut self, pointer: i32) {
        self.end_pointer = pointer;
    }

    /// Returns the current handler state.
    #[must_use] 
    pub const fn state(&self) -> ExceptionHandlingState {
        self.state
    }

    /// Updates the current handler state.
    pub fn set_state(&mut self, state: ExceptionHandlingState) {
        self.state = state;
    }

    /// Returns `true` when this context includes a `catch` block.
    #[must_use] 
    pub const fn has_catch(&self) -> bool {
        self.catch_pointer >= 0
    }

    /// Returns `true` when this context includes a `finally` block.
    #[must_use] 
    pub const fn has_finally(&self) -> bool {
        self.finally_pointer >= 0
    }

    /// Indicates whether the VM is currently executing the exception handler
    /// (either `catch` or `finally`).
    #[must_use] 
    pub const fn is_in_exception(&self) -> bool {
        matches!(
            self.state,
            ExceptionHandlingState::Catch | ExceptionHandlingState::Finally
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_initialises_fields() {
        let ctx = ExceptionHandlingContext::new(10, 20);
        assert_eq!(ctx.catch_pointer(), 10);
        assert_eq!(ctx.finally_pointer(), 20);
        assert_eq!(ctx.end_pointer(), -1);
        assert_eq!(ctx.state(), ExceptionHandlingState::Try);
        assert!(ctx.has_catch());
        assert!(ctx.has_finally());
        assert!(!ctx.is_in_exception());
    }

    #[test]
    fn setters_update_state() {
        let mut ctx = ExceptionHandlingContext::new(-1, 30);
        assert!(!ctx.has_catch());
        assert!(ctx.has_finally());

        ctx.set_end_pointer(100);
        assert_eq!(ctx.end_pointer(), 100);

        ctx.set_state(ExceptionHandlingState::Catch);
        assert_eq!(ctx.state(), ExceptionHandlingState::Catch);
        assert!(ctx.is_in_exception());

        ctx.set_state(ExceptionHandlingState::Finally);
        assert_eq!(ctx.state(), ExceptionHandlingState::Finally);
    }
}
