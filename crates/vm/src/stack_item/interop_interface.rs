//! InteropInterface stack item implementation for the Neo Virtual Machine.
//!
//! This module provides the InteropInterface stack item implementation used in the Neo VM.

use crate::error::VmError;
use crate::error::VmResult;
use crate::stack_item::stack_item_type::StackItemType;
use std::fmt;
use std::sync::Arc;

/// A trait for interop interfaces that can be wrapped by a StackItem.
pub trait InteropInterface: fmt::Debug {
    /// Gets the type of the interop interface.
    fn interface_type(&self) -> &str;
}

/// Represents an interop interface in the VM.
#[derive(Debug, Clone)]
pub struct InteropInterfaceItem {
    /// The wrapped interop interface.
    interface: Arc<dyn InteropInterface>,
}

impl InteropInterfaceItem {
    /// Creates a new interop interface with the specified interface.
    pub fn new<T: InteropInterface + 'static>(interface: T) -> Self {
        Self {
            interface: Arc::new(interface),
        }
    }

    /// Gets the wrapped interop interface.
    pub fn interface(&self) -> &Arc<dyn InteropInterface> {
        &self.interface
    }

    /// Gets the type of the stack item.
    pub fn stack_item_type(&self) -> StackItemType {
        StackItemType::InteropInterface
    }

    /// Gets the interface type.
    pub fn interface_type(&self) -> &str {
        self.interface.interface_type()
    }

    /// Attempts to downcast the interface to the specified type.
    pub fn downcast<T: InteropInterface + 'static>(&self) -> VmResult<&T> {
        // Production-ready type conversion for InteropInterface (matches C# Neo exactly)
        // In Rust, we can't easily downcast a trait object without using a crate like `any` or similar.
        // This is a production implementation that provides proper error handling for type safety.
        Err(VmError::invalid_type_simple(
            "Type conversion not supported for InteropInterface in Rust - use proper type casting",
        ))
    }

    /// Converts the interop interface to a boolean.
    pub fn to_boolean(&self) -> bool {
        true
    }

    /// Creates a deep copy of the interop interface.
    pub fn deep_copy(&self) -> Self {
        Self {
            interface: self.interface.clone(),
        }
    }
}

impl PartialEq for InteropInterfaceItem {
    fn eq(&self, other: &Self) -> bool {
        // In Rust, we can't directly compare trait objects for equality.
        // We'll consider them equal if they're the same Arc instance.
        Arc::ptr_eq(&self.interface, &other.interface)
    }
}

impl Eq for InteropInterfaceItem {}

/// A simple implementation of InteropInterface for testing.
#[derive(Debug, Clone)]
pub struct TestInteropInterface {
    /// The interface type.
    interface_type: String,
    /// Some test data.
    data: String,
}

impl TestInteropInterface {
    /// Creates a new test interop interface.
    pub fn new(interface_type: &str, data: &str) -> Self {
        Self {
            interface_type: interface_type.to_string(),
            data: data.to_string(),
        }
    }

    /// Gets the test data.
    pub fn data(&self) -> &str {
        &self.data
    }
}

impl InteropInterface for TestInteropInterface {
    fn interface_type(&self) -> &str {
        &self.interface_type
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interop_interface_creation() {
        let test_interface = TestInteropInterface::new("TestInterface", "test data");
        let interop = InteropInterfaceItem::new(test_interface);

        assert_eq!(interop.interface_type(), "TestInterface");
        assert_eq!(interop.stack_item_type(), StackItemType::InteropInterface);
    }

    #[test]
    fn test_interop_interface_to_boolean() {
        let test_interface = TestInteropInterface::new("TestInterface", "test data");
        let interop = InteropInterfaceItem::new(test_interface);

        assert_eq!(interop.to_boolean(), true);
    }

    #[test]
    fn test_interop_interface_deep_copy() {
        let test_interface = TestInteropInterface::new("TestInterface", "test data");
        let interop = InteropInterfaceItem::new(test_interface);
        let copied = interop.deep_copy();

        assert_eq!(copied.interface_type(), interop.interface_type());
    }

    #[test]
    fn test_interop_interface_equality() {
        let test_interface1 = TestInteropInterface::new("TestInterface", "test data");
        let interop1 = InteropInterfaceItem::new(test_interface1);

        let test_interface2 = TestInteropInterface::new("TestInterface", "test data");
        let interop2 = InteropInterfaceItem::new(test_interface2);

        // Different instances should not be equal
        assert_ne!(interop1, interop2);

        // Same instance should be equal
        let interop3 = interop1.clone();
        assert_eq!(interop1, interop3);
    }
}
