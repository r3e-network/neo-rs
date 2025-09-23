//! Exception raised when a malformed script is encountered during parsing.
//!
//! Direct port of `Neo.VM/BadScriptException.cs`.

use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};

/// Represents the exception thrown when a bad script is parsed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BadScriptException {
    message: Option<String>,
}

impl BadScriptException {
    /// Creates a new exception without a message.
    pub fn new() -> Self {
        Self { message: None }
    }

    /// Creates a new exception with the specified message.
    pub fn with_message<M: Into<String>>(message: M) -> Self {
        Self {
            message: Some(message.into()),
        }
    }

    /// Returns the message, if any, associated with the exception.
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }
}

impl Display for BadScriptException {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match &self.message {
            Some(message) => write!(f, "{message}"),
            None => write!(f, "A malformed script was encountered."),
        }
    }
}

impl StdError for BadScriptException {}
