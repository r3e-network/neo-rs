use neo_vm::types::StackItem;

/// Represents the object that can be converted to and from `StackItem`
/// and allows you to specify whether a verification is required.
pub trait InteroperableVerifiable: Interoperable {
    /// Convert a `StackItem` to the current object.
    ///
    /// # Arguments
    ///
    /// * `stack_item` - The `StackItem` to convert.
    /// * `verify` - Verify the content
    fn from_stack_item(&mut self, stack_item: &StackItem, verify: bool);
}
