//! RPC session iterator retention and disposal helpers.

use neo_error::CoreResult;
use neo_execution::iterators::StorageIterator;
use neo_execution::iterators::iterator::StorageIterator as _;
use neo_vm::stack_item::StackItem;

/// Trait representing an iterator stored within an RPC session.
pub trait SessionIterator: Send {
    /// Advance the iterator to the next item.
    fn next(&mut self) -> bool;
    /// Return the current item.
    fn value(&self) -> CoreResult<StackItem>;
    /// Release any resources owned by the iterator.
    fn dispose(&mut self);
}

/// Wrapper storing iterator instances with automatic disposal.
pub(super) struct IteratorEntry {
    pub(super) inner: Box<dyn SessionIterator>,
}

impl IteratorEntry {
    pub(super) fn next(&mut self) -> bool {
        self.inner.next()
    }

    pub(super) fn value(&self) -> CoreResult<StackItem> {
        self.inner.value()
    }

    fn dispose(&mut self) {
        self.inner.dispose();
    }
}

impl Drop for IteratorEntry {
    fn drop(&mut self) {
        self.dispose();
    }
}

#[derive(Debug)]
pub(super) struct StorageSessionIterator {
    iterator: StorageIterator,
}

impl StorageSessionIterator {
    pub(super) const fn new(iterator: StorageIterator) -> Self {
        Self { iterator }
    }
}

impl SessionIterator for StorageSessionIterator {
    fn next(&mut self) -> bool {
        self.iterator.next()
    }

    fn value(&self) -> CoreResult<StackItem> {
        self.iterator.value()
    }

    fn dispose(&mut self) {
        self.iterator.dispose();
    }
}
