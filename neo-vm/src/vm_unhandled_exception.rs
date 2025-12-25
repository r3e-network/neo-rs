//! VM unhandled exception implementation.
//!
//! This module provides the VMUnhandledException functionality exactly matching C# Neo.VM.VMUnhandledException.

// Matches C# using directives exactly:
// using Neo.VM.Types;
// using System;
// using System.Text;
// using Array = Neo.VM.Types.Array;

use crate::stack_item::StackItem;
use std::error::Error;
use std::fmt;

/// namespace Neo.VM -> public class VMUnhandledException : Exception
/// Represents an unhandled exception in the VM.
/// Thrown when there is an exception in the VM that is not caught by any script.
#[derive(Debug, Clone)]
pub struct VMUnhandledException {
    /// The unhandled exception in the VM.
    /// public StackItem ExceptionObject { get; }
    pub exception_object: StackItem,
    message: String,
}

impl VMUnhandledException {
    /// Initializes a new instance of the VMUnhandledException class.
    /// public VMUnhandledException(StackItem ex) : base(GetExceptionMessage(ex))
    pub fn new(ex: StackItem) -> Self {
        let message = Self::get_exception_message(&ex);
        Self {
            exception_object: ex,
            message,
        }
    }

    /// private static string GetExceptionMessage(StackItem e)
    fn get_exception_message(e: &StackItem) -> String {
        let mut message = String::from("An unhandled exception was thrown.");

        let mut bytes_opt = match e {
            StackItem::ByteString(bytes) => Some(bytes.clone()),
            _ => None,
        };

        if bytes_opt.is_none() {
            if let Ok(array) = e.as_array() {
                if let Some(StackItem::ByteString(bytes)) = array.first() {
                    bytes_opt = Some(bytes.clone());
                }
            }
        }

        if let Some(bytes) = bytes_opt {
            message.push(' ');
            message.push_str(&String::from_utf8_lossy(&bytes));
        }

        message
    }
}

impl fmt::Display for VMUnhandledException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for VMUnhandledException {}
