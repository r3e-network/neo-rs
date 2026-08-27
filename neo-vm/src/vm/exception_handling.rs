//! Shared NeoVM exception handling frame metadata.

/// Indicates the state of a NeoVM exception handling context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExceptionHandlingState {
    /// The VM is currently executing the try region.
    Try,
    /// The VM is currently executing the catch region.
    Catch,
    /// The VM is currently executing the finally region.
    Finally,
}

/// Represents the try/catch/finally context pushed on the invocation stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExceptionHandlingContext {
    catch_pointer: i32,
    finally_pointer: i32,
    end_pointer: i32,
    state: ExceptionHandlingState,
}

impl ExceptionHandlingContext {
    /// Creates a new exception handling context.
    #[must_use]
    pub const fn new(catch_pointer: i32, finally_pointer: i32) -> Self {
        Self {
            catch_pointer,
            finally_pointer,
            end_pointer: -1,
            state: ExceptionHandlingState::Try,
        }
    }

    /// Returns the position of the catch block. `-1` means no catch block.
    #[must_use]
    pub const fn catch_pointer(&self) -> i32 {
        self.catch_pointer
    }

    /// Returns the position of the finally block. `-1` means no finally block.
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

    /// Returns `true` when this context includes a catch block.
    #[must_use]
    pub const fn has_catch(&self) -> bool {
        self.catch_pointer >= 0
    }

    /// Returns `true` when this context includes a finally block.
    #[must_use]
    pub const fn has_finally(&self) -> bool {
        self.finally_pointer >= 0
    }

    /// Indicates whether the VM is currently executing an exception handler.
    #[must_use]
    pub const fn is_in_exception(&self) -> bool {
        matches!(
            self.state,
            ExceptionHandlingState::Catch | ExceptionHandlingState::Finally
        )
    }
}
