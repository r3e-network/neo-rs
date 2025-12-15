use neo_vm::stack_item::InteropInterface as VmInteropInterface;
use std::any::Any;

#[derive(Debug)]
pub struct IteratorInterop {
    id: u32,
}

impl IteratorInterop {
    pub fn new(id: u32) -> Self {
        Self { id }
    }

    pub fn id(&self) -> u32 {
        self.id
    }
}

impl VmInteropInterface for IteratorInterop {
    fn interface_type(&self) -> &str {
        "IIterator"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
