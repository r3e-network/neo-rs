use neo_vm::stack_item::StackItem;
use crate::neo_contract::application_engine::ApplicationEngine;
use crate::neo_contract::call_flags::CallFlags;
use crate::neo_contract::interop_descriptor::InteropDescriptor;
use crate::register_syscall;

impl ApplicationEngine {
    /// The `InteropDescriptor` of System.Iterator.Next.
    /// Advances the iterator to the next element of the collection.
    pub const SYSTEM_ITERATOR_NEXT: InteropDescriptor = register_syscall(
        "System.Iterator.Next",
        ApplicationEngine::iterator_next,
        1 << 15,
        CallFlags::NONE
    );

    /// The `InteropDescriptor` of System.Iterator.Value.
    /// Gets the element in the collection at the current position of the iterator.
    pub const SYSTEM_ITERATOR_VALUE: InteropDescriptor = register_syscall(
        "System.Iterator.Value",
        ApplicationEngine::iterator_value,
        1 << 4,
        CallFlags::NONE
    );

    /// The implementation of System.Iterator.Next.
    /// Advances the iterator to the next element of the collection.
    ///
    /// # Arguments
    ///
    /// * `iterator` - The iterator to be advanced.
    ///
    /// # Returns
    ///
    /// `true` if the iterator was successfully advanced to the next element; `false` if the iterator has passed the end of the collection.
    fn iterator_next(engine: &mut ApplicationEngine) -> Result<bool, Box<dyn std::error::Error>> {
        let iterator = engine.pop_as::<NeoIterator>()?;
        Ok(iterator.next())
    }

    /// The implementation of System.Iterator.Value.
    /// Gets the element in the collection at the current position of the iterator.
    ///
    /// # Arguments
    ///
    /// * `iterator` - The iterator to be used.
    ///
    /// # Returns
    ///
    /// The element in the collection at the current position of the iterator.
    fn iterator_value(engine: &mut ApplicationEngine) -> Result<StackItem, Box<dyn std::error::Error>> {
        let iterator = engine.pop_as::<NeoIterator>()?;
        Ok(iterator.value(engine.reference_counter.clone()))
    }
}
