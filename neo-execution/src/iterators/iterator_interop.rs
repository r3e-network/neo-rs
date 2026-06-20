use neo_vm::stack_item::InteropInterface as VmInteropInterface;
use std::any::Any;

#[derive(Debug)]
/// VM interop handle that points to an engine-managed storage iterator.
pub struct IteratorInterop {
    id: u32,
}

impl IteratorInterop {
    /// Creates an interop handle for a storage iterator id.
    pub fn new(id: u32) -> Self {
        Self { id }
    }

    /// Returns the engine-managed storage iterator id.
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
