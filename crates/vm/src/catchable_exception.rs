//! Exceptions that can be caught within VM scripts.
//!
//! Port of `Neo.VM/CatchableException.cs` from the C# reference implementation.

use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};

/// An exception type that smart contracts are allowed to intercept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatchableException {
    message: String,
}

impl CatchableException {
    /// Creates a new catchable exception with the provided message.
    pub fn new<M: Into<String>>(message: M) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns the exception message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl Display for CatchableException {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl StdError for CatchableException {}
