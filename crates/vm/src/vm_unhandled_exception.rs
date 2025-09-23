//! Unhandled exception wrapper for the Neo VM.
//!
//! Mirrors `Neo.VM/VMUnhandledException.cs` from the C# codebase.

use crate::stack_item::StackItem;
use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};

/// Represents an unhandled exception propagated out of the VM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VMUnhandledException {
    message: String,
    exception_object: StackItem,
}

impl VMUnhandledException {
    /// Creates a new wrapper for the supplied exception object.
    pub fn new(exception: StackItem) -> Self {
        let message = build_exception_message(&exception);
        Self {
            message,
            exception_object: exception,
        }
    }

    /// Returns the underlying VM exception object.
    pub fn exception_object(&self) -> &StackItem {
        &self.exception_object
    }
}

impl Display for VMUnhandledException {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl StdError for VMUnhandledException {}

fn build_exception_message(exception: &StackItem) -> String {
    let mut message = String::from("An unhandled exception was thrown.");

    if let Some(text) = extract_payload(exception) {
        message.push(' ');
        message.push_str(&text);
    }

    message
}

fn extract_payload(exception: &StackItem) -> Option<String> {
    match exception {
        StackItem::ByteString(bytes) => String::from_utf8(bytes.clone()).ok(),
        StackItem::Array(items) if !items.is_empty() => {
            items.items().first().and_then(|first| match first {
                StackItem::ByteString(bytes) => String::from_utf8(bytes.clone()).ok(),
                _ => None,
            })
        }
        _ => None,
    }
}
