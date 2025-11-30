//! Catchable exception implementation.
//!
//! This module provides the CatchableException functionality exactly matching C# Neo.VM.CatchableException.

// Matches C# using directives exactly:
// using System;

use std::error::Error;
use std::fmt;

/// namespace Neo.VM -> public class CatchableException : Exception

#[derive(Debug, Clone)]
pub struct CatchableException {
    message: String,
}

impl CatchableException {
    /// public CatchableException(string message) : base(message)
    pub fn new(message: String) -> Self {
        Self { message }
    }
}

impl fmt::Display for CatchableException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for CatchableException {}
