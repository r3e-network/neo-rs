use crate::neo_vm::stack_item::InteropInterface as VmInteropInterface;
use std::any::Any;

/// Interop interface wrapper exposing a storage iterator to the VM (C# `StorageIterator`).
#[derive(Debug)]
pub struct IteratorInterop {
    id: u32,
}

impl IteratorInterop {
    /// Creates an iterator interop with the given engine-side id.
    pub fn new(id: u32) -> Self {
        Self { id }
    }

    /// Returns the engine-side iterator id.
    pub fn id(&self) -> u32 {
        self.id
    }
}

impl VmInteropInterface for IteratorInterop {
    fn interface_type(&self) -> &str {
        "StorageIterator"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
