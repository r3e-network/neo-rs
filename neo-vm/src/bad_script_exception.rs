//! Bad script exception implementation.
//!
//! This module provides the BadScriptException functionality exactly matching C# Neo.VM.BadScriptException.

// Matches C# using directives exactly:
// using System;

use std::error::Error;
use std::fmt;

/// namespace Neo.VM -> public class `BadScriptException` : Exception
/// Represents the exception thrown when the bad script is parsed.
#[derive(Debug, Clone)]
pub struct BadScriptException {
    message: String,
}

impl BadScriptException {
    /// Initializes a new instance of the `BadScriptException` class.
    /// public `BadScriptException()` { }
    #[must_use] 
    pub const fn new() -> Self {
        Self {
            message: String::new(),
        }
    }

    /// Initializes a new instance of the `BadScriptException` class with a specified error message.
    /// public BadScriptException(string message) : base(message) { }
    #[must_use] 
    pub const fn with_message(message: String) -> Self {
        Self { message }
    }
}

impl fmt::Display for BadScriptException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for BadScriptException {}

impl Default for BadScriptException {
    fn default() -> Self {
        Self::new()
    }
}
