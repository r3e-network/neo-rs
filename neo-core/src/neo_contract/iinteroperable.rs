use std::cell::RefCell;
use std::rc::Rc;
use neo_vm::vm_types::reference_counter::ReferenceCounter;
use neo_vm::vm_types::stack_item::StackItem;

/// Represents an object that can be converted to and from `StackItem`.
// TODO: clone method need further check since its related to reference counter, can not directly derive from Clone
pub trait IInteroperable: Default {
    type Error;

    /// Converts a `StackItem` to the current object.
    ///
    /// # Arguments
    ///
    /// * `stack_item` - The `StackItem` to convert.
    ///
    /// # Returns
    ///
    /// A `Result` containing the converted object or an error.
    fn from_stack_item(stack_item: &Rc<StackItem>) -> Result<Self, Self::Error>;

    /// Converts the current object to a `StackItem`.
    ///
    /// # Arguments
    ///
    /// * `reference_counter` - An optional `ReferenceCounter` used by the `StackItem`.
    ///
    /// # Returns
    ///
    /// A `Result` containing the converted `StackItem` or an error.
    fn to_stack_item(&self, reference_counter: Option<&mut Rc<RefCell<ReferenceCounter>>>) -> Result<Rc<StackItem>, Self::Error>;

    /// Creates a clone of the current object.
    ///
    /// # Returns
    ///
    /// A `Result` containing the cloned object or an error.
    fn clone(&self) -> Result<Self, Self::Error>
    where
        Self: Sized,
    {
        Self::from_stack_item(&self.to_stack_item(None)?)
    }

    /// Creates a new instance from a replica of another `IInteroperable` object.
    ///
    /// # Arguments
    ///
    /// * `replica` - The `IInteroperable` object to replicate.
    ///
    /// # Returns
    ///
    /// A `Result` containing the new instance or an error.
    fn from_replica(replica: &dyn IInteroperable<Error=Self::Error>) -> Result<Self, Self::Error> {
        Self::from_stack_item(&replica.to_stack_item(None)?)
    }
}
