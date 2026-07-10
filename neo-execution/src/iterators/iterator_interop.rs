use neo_vm::stack_item::InteropInterface;

/// VM interop handle that points to an engine-managed storage iterator.
pub type IteratorInterop = InteropInterface;

/// Constructors for storage iterator VM interop handles.
pub trait IteratorInteropExt {
    /// Creates an interop handle for a storage iterator id.
    fn new(id: u32) -> Self;

    /// Returns the engine-managed storage iterator id.
    fn id(&self) -> u32;
}

impl IteratorInteropExt for InteropInterface {
    fn new(id: u32) -> Self {
        InteropInterface::iterator(id)
    }

    fn id(&self) -> u32 {
        self.iterator_id().unwrap_or(0)
    }
}
