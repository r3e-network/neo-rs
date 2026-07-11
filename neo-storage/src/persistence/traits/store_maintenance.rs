//! Node-local storage maintenance operations.
//!
//! Maintenance metadata is physically isolated from Neo contract storage so
//! operational checkpoints cannot enter typed scans, store dumps, or state
//! root calculation. A maintenance batch lets persistent backends apply data
//! mutations and metadata updates at one durable transaction boundary.

use std::collections::BTreeMap;

/// Ordered data and node-local metadata operations committed atomically.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StoreMaintenanceBatch {
    data: BTreeMap<Vec<u8>, Option<Vec<u8>>>,
    metadata: BTreeMap<Vec<u8>, Option<Vec<u8>>>,
}

impl StoreMaintenanceBatch {
    /// Creates an empty maintenance batch.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            data: BTreeMap::new(),
            metadata: BTreeMap::new(),
        }
    }

    /// Returns whether the batch contains no operations.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty() && self.metadata.is_empty()
    }

    /// Inserts or replaces one normal data-table value.
    pub fn put_data(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.data.insert(key, Some(value));
    }

    /// Deletes one normal data-table value.
    pub fn delete_data(&mut self, key: Vec<u8>) {
        self.data.insert(key, None);
    }

    /// Inserts or replaces one isolated maintenance-metadata value.
    pub fn put_metadata(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.metadata.insert(key, Some(value));
    }

    /// Deletes one isolated maintenance-metadata value.
    pub fn delete_metadata(&mut self, key: Vec<u8>) {
        self.metadata.insert(key, None);
    }

    /// Returns ordered normal data-table operations.
    pub fn data_operations(&self) -> impl ExactSizeIterator<Item = (&[u8], Option<&[u8]>)> {
        self.data
            .iter()
            .map(|(key, value)| (key.as_slice(), value.as_deref()))
    }

    /// Returns ordered isolated maintenance-metadata operations.
    pub fn metadata_operations(&self) -> impl ExactSizeIterator<Item = (&[u8], Option<&[u8]>)> {
        self.metadata
            .iter()
            .map(|(key, value)| (key.as_slice(), value.as_deref()))
    }
}
