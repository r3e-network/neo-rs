//! RPC session iterator retention and disposal helpers.

use std::sync::Arc;

use neo_error::CoreResult;
use neo_execution::iterators::IteratorInterop;
use neo_execution::iterators::StorageIterator;
use neo_execution::iterators::iterator::StorageIterator as _;
use neo_vm::stack_item::{InteropInterface as VmInteropInterface, StackItem};
use uuid::Uuid;

use super::Session;

/// Trait representing an iterator stored within an RPC session.
pub(super) trait SessionIterator: Send {
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

impl Session {
    /// Return whether this session currently retains any iterators.
    pub fn has_iterators(&self) -> bool {
        !self.iterators.lock().is_empty()
    }

    /// Register a VM iterator interface and return the stable RPC iterator id.
    ///
    /// Re-registering the same VM iterator returns its existing UUID.
    pub fn register_iterator_interface(
        &self,
        interface: &Arc<dyn VmInteropInterface>,
    ) -> Option<Uuid> {
        let iterator_interop = interface.as_any().downcast_ref::<IteratorInterop>()?;
        let iterator_id = iterator_interop.id();

        if let Some(existing) = self.iterator_lookup.lock().get(&iterator_id).copied() {
            return Some(existing);
        }

        let iterator = {
            let mut engine_guard = self.engine.lock();
            engine_guard.take_storage_iterator(iterator_id)?
        };

        let uuid = Uuid::new_v4();
        self.iterators.lock().insert(
            uuid,
            IteratorEntry {
                inner: Box::new(StorageSessionIterator::new(iterator)),
            },
        );
        self.iterator_lookup.lock().insert(iterator_id, uuid);

        Some(uuid)
    }

    /// Read up to `count` items from a previously registered iterator.
    pub fn traverse_iterator(
        &self,
        iterator_id: &Uuid,
        count: usize,
    ) -> Result<Vec<StackItem>, String> {
        let mut iterators = self.iterators.lock();
        let Some(entry) = iterators.get_mut(iterator_id) else {
            return Err("Unknown iterator".to_string());
        };

        let mut remaining = count;
        let mut values = Vec::new();
        while remaining > 0 && entry.next() {
            values.push(entry.value().map_err(|error| error.to_string())?);
            remaining -= 1;
        }
        Ok(values)
    }
}
