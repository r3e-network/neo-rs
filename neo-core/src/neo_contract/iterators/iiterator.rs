use NeoRust::types::StackItem;

/// Represents iterators in smart contracts.
pub trait Iterator {
    /// Advances the iterator to the next element of the collection.
    ///
    /// # Returns
    ///
    /// `true` if the iterator was successfully advanced to the next element;
    /// `false` if the iterator has passed the end of the collection.
    fn next(&mut self) -> bool;

    /// Gets the element in the collection at the current position of the iterator.
    ///
    /// # Arguments
    ///
    /// * `reference_counter` - A reference counter for managing object lifetimes.
    ///
    /// # Returns
    ///
    /// The element in the collection at the current position of the iterator.
    fn value(&self, reference_counter: &mut ReferenceCounter) -> StackItem;
}