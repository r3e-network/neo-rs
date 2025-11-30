//! Exception handling state for the Neo Virtual Machine.
//!
//! Ports `Neo.VM/ExceptionHandlingState.cs` verbatim so the VM can track the
//! lifecycle of try/catch/finally blocks exactly like the C# reference
//! implementation.

/// Indicates the state of an [`ExceptionHandlingContext`](crate::exception_handling_context::ExceptionHandlingContext).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExceptionHandlingState {
    /// The VM is currently executing the `try` region.
    Try,
    /// The VM is currently executing the `catch` region.
    Catch,
    /// The VM is currently executing the `finally` region.
    Finally,
}
