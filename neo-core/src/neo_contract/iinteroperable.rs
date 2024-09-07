use neo::vm::StackItem;
use neo::vm::ReferenceCounter;

/// Represents the object that can be converted to and from `StackItem`.
pub trait IInteroperable {
    /// Convert a `StackItem` to the current object.
    ///
    /// # Arguments
    ///
    /// * `stack_item` - The `StackItem` to convert.
    fn from_stack_item(&mut self, stack_item: StackItem);

    /// Convert the current object to a `StackItem`.
    ///
    /// # Arguments
    ///
    /// * `reference_counter` - The `ReferenceCounter` used by the `StackItem`.
    ///
    /// # Returns
    ///
    /// The converted `StackItem`.
    fn to_stack_item(&self, reference_counter: Option<&ReferenceCounter>) -> StackItem;

    fn clone(&self) -> Box<dyn IInteroperable>
    where
        Self: Sized,
    {
        let mut result = Box::new(Self::default());
        result.from_stack_item(self.to_stack_item(None));
        result
    }

    fn from_replica(&mut self, replica: &dyn IInteroperable) {
        self.from_stack_item(replica.to_stack_item(None));
    }
}
