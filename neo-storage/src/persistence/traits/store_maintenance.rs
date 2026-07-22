//! Node-local storage maintenance operations.
//!
//! Maintenance metadata is physically isolated from Neo contract storage so
//! operational checkpoints cannot enter typed scans, store dumps, or state
//! root calculation. A maintenance batch lets persistent backends apply data
//! mutations and metadata updates at one durable transaction boundary.

use std::collections::BTreeMap;

use crate::persistence::table::{IntoTableBytes, Table, TableEncode, TableNamespace};
use crate::{StorageError, StorageResult};

/// Exact raw value that must remain unchanged until a guarded maintenance
/// transaction commits.
///
/// Guards are evaluated by the backend inside the same write transaction as
/// the maintenance mutation. They are intended for offline migrations and
/// recovery tooling that must fail closed if a canonical or service-owned
/// pointer changes after preflight.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreValueGuard {
    key: Vec<u8>,
    expected: Option<Vec<u8>>,
}

impl StoreValueGuard {
    /// Requires `key` to retain the exact expected value.
    #[must_use]
    pub fn present(key: Vec<u8>, expected: Vec<u8>) -> Self {
        Self {
            key,
            expected: Some(expected),
        }
    }

    /// Requires `key` to remain absent.
    #[must_use]
    pub fn absent(key: Vec<u8>) -> Self {
        Self {
            key,
            expected: None,
        }
    }

    /// Raw key checked by the guarded transaction.
    #[must_use]
    pub fn key(&self) -> &[u8] {
        &self.key
    }

    /// Exact value required by the guarded transaction, or `None` for absence.
    #[must_use]
    pub fn expected(&self) -> Option<&[u8]> {
        self.expected.as_deref()
    }
}

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

    /// Inserts or replaces one statically typed logical-table value.
    pub fn put<T: Table>(&mut self, key: &T::Key, value: &T::Value) -> StorageResult<()> {
        let key = <T::KeyCodec as TableEncode<T::Key>>::encode(key)
            .map_err(|error| table_codec_error::<T>("encode key", error))?
            .into_table_bytes();
        let value = <T::ValueCodec as TableEncode<T::Value>>::encode(value)
            .map_err(|error| table_codec_error::<T>("encode value", error))?
            .into_table_bytes();
        self.operations_mut(T::NAMESPACE).insert(key, Some(value));
        Ok(())
    }

    /// Deletes one statically typed logical-table value.
    pub fn delete<T: Table>(&mut self, key: &T::Key) -> StorageResult<()> {
        let key = <T::KeyCodec as TableEncode<T::Key>>::encode(key)
            .map_err(|error| table_codec_error::<T>("encode key", error))?
            .into_table_bytes();
        self.operations_mut(T::NAMESPACE).insert(key, None);
        Ok(())
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

    fn operations_mut(
        &mut self,
        namespace: TableNamespace,
    ) -> &mut BTreeMap<Vec<u8>, Option<Vec<u8>>> {
        match namespace {
            TableNamespace::Data => &mut self.data,
            TableNamespace::Maintenance => &mut self.metadata,
        }
    }
}

fn table_codec_error<T: Table>(operation: &'static str, error: StorageError) -> StorageError {
    StorageError::serialization(format!("{operation} for table {}: {error}", T::NAME))
}
