//! Exception handling primitives mirroring the C# Neo VM implementation.
//!
//! The reference node models the try/catch/finally machinery with a very small
//! data structure: the `ExceptionHandlingContext`.  Each context simply tracks
//! the absolute instruction pointers for the catch and finally blocks together
//! with a mutable `EndPointer` that is set when executing `ENDTRY`.  The state of
//! the handler is represented by the `ExceptionHandlingState` enum.

/// Indicates the phase of execution for a `try` handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExceptionHandlingState {
    /// The VM is executing the `try` body.
    Try,
    /// The VM is executing the `catch` block.
    Catch,
    /// The VM is executing the `finally` block.
    Finally,
}

/// Captures the metadata required to resume execution inside a try/catch/finally block.
///
/// This is a direct translation of `Neo.VM.ExceptionHandlingContext` from the C#
/// codebase: the VM stores the absolute offsets of the catch and finally blocks,
/// exposes helper accessors (`HasCatch`, `HasFinally`) and keeps track of the
/// current state.  The `EndPointer` property is initialised to `-1` and updated
/// when the VM encounters an `ENDTRY` opcode.
#[derive(Debug, Clone)]
pub struct ExceptionHandlingContext {
    pub catch_pointer: i32,
    pub finally_pointer: i32,
    pub end_pointer: i32,
    pub state: ExceptionHandlingState,
}

impl ExceptionHandlingContext {
    /// Creates a new handler context with the supplied catch/finally pointers.
    ///
    /// Offsets that are not present in the bytecode are represented with `-1`
    /// exactly like the C# implementation.
    pub fn new(catch_pointer: i32, finally_pointer: i32) -> Self {
        Self {
            catch_pointer,
            finally_pointer,
            end_pointer: -1,
            state: ExceptionHandlingState::Try,
        }
    }

    /// Absolute instruction pointer for the `catch` block (or `-1` if absent).
    pub fn catch_pointer(&self) -> i32 {
        self.catch_pointer
    }

    /// Absolute instruction pointer for the `finally` block (or `-1` if absent).
    pub fn finally_pointer(&self) -> i32 {
        self.finally_pointer
    }

    /// Absolute instruction pointer for the location after the handler.
    pub fn end_pointer(&self) -> i32 {
        self.end_pointer
    }

    /// Updates the `end_pointer` when processing `ENDTRY`.
    pub fn set_end_pointer(&mut self, pointer: i32) {
        self.end_pointer = pointer;
    }

    /// Indicates whether the handler defines a catch block.
    pub fn has_catch(&self) -> bool {
        self.catch_pointer >= 0
    }

    /// Indicates whether the handler defines a finally block.
    pub fn has_finally(&self) -> bool {
        self.finally_pointer >= 0
    }

    /// Returns the current execution state of the handler.
    pub fn state(&self) -> ExceptionHandlingState {
        self.state
    }

    /// Sets the execution state of the handler.
    pub fn set_state(&mut self, state: ExceptionHandlingState) {
        self.state = state;
    }

    /// Returns `true` when the handler is executing either the catch or finally block.
    pub fn is_in_exception(&self) -> bool {
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
    fn default_state_matches_csharp() {
        let ctx = ExceptionHandlingContext::new(12, 34);
        assert_eq!(ctx.catch_pointer(), 12);
        assert_eq!(ctx.finally_pointer(), 34);
        assert_eq!(ctx.end_pointer(), -1);
        assert!(ctx.has_catch());
        assert!(ctx.has_finally());
        assert_eq!(ctx.state(), ExceptionHandlingState::Try);
    }

    #[test]
    fn absent_handlers_are_reported_correctly() {
        let ctx = ExceptionHandlingContext::new(-1, -1);
        assert!(!ctx.has_catch());
        assert!(!ctx.has_finally());
    }

    #[test]
    fn setters_behave_like_reference_node() {
        let mut ctx = ExceptionHandlingContext::new(10, -1);
        assert!(ctx.has_catch());
        assert!(!ctx.has_finally());

        ctx.set_end_pointer(123);
        assert_eq!(ctx.end_pointer(), 123);

        ctx.set_state(ExceptionHandlingState::Catch);
        assert_eq!(ctx.state(), ExceptionHandlingState::Catch);

        ctx.set_state(ExceptionHandlingState::Finally);
        assert_eq!(ctx.state(), ExceptionHandlingState::Finally);
    }
}
