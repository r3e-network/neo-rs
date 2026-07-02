//! # neo-storage::persistence::table
//!
//! Typed table boundaries over raw byte-key stores.
//!
//! ## Boundary
//!
//! This module assigns compile-time table names and codecs to existing raw
//! key/value bytes. It must not change Neo storage serialization, MPT hashing,
//! or backend-specific write ordering.
//!
//! ## Contents
//!
//! - `Table`: table metadata and key/value codec types.
//! - `TableCodec`: byte-preserving encode/decode contract for table fields.
//! - `TableReader`: typed read adapter over `RawReadOnlyStore`.
//! - `StoreTableRead`: common table read capability.

use super::RawReadOnlyStore;
use crate::{StorageError, StorageItem, StorageKey, StorageResult};
use std::marker::PhantomData;

/// Byte codec used by typed storage tables.
///
/// Implementations must preserve the existing persisted Neo bytes. This trait
/// is a table-boundary adapter, not a new consensus serialization format.
pub trait TableCodec: Sized {
    /// Encodes this value into its persisted byte representation.
    fn encode(&self) -> Vec<u8>;

    /// Decodes this value from persisted bytes.
    fn decode(bytes: &[u8]) -> StorageResult<Self>;
}

/// Compile-time metadata for a typed storage table.
pub trait Table {
    /// Key type used by this table.
    type Key: TableCodec;

    /// Value type used by this table.
    type Value: TableCodec;

    /// Stable table name used in diagnostics and metrics labels.
    const NAME: &'static str;
}

/// Typed read adapter over a raw byte-key store.
pub struct TableReader<'a, T: Table> {
    store: &'a dyn RawReadOnlyStore,
    _table: PhantomData<T>,
}

impl<'a, T: Table> TableReader<'a, T> {
    /// Creates a typed reader for `T` over an existing raw store.
    #[must_use]
    pub const fn new(store: &'a dyn RawReadOnlyStore) -> Self {
        Self {
            store,
            _table: PhantomData,
        }
    }
}

/// Read capability for a typed storage table.
pub trait StoreTableRead<T: Table> {
    /// Reads and decodes a row by typed key.
    fn get(&self, key: &T::Key) -> StorageResult<Option<T::Value>>;
}

impl<T: Table> StoreTableRead<T> for TableReader<'_, T> {
    fn get(&self, key: &T::Key) -> StorageResult<Option<T::Value>> {
        let raw_key = key.encode();
        let Some(raw_value) = self.store.try_get_bytes(&raw_key) else {
            return Ok(None);
        };

        T::Value::decode(&raw_value).map(Some).map_err(|err| {
            StorageError::invalid_data(format!(
                "failed to decode value from table '{}': {err}",
                T::NAME
            ))
        })
    }
}

impl TableCodec for StorageKey {
    fn encode(&self) -> Vec<u8> {
        self.to_array()
    }

    fn decode(bytes: &[u8]) -> StorageResult<Self> {
        Ok(Self::from_bytes(bytes))
    }
}

impl TableCodec for StorageItem {
    fn encode(&self) -> Vec<u8> {
        self.to_value()
    }

    fn decode(bytes: &[u8]) -> StorageResult<Self> {
        Ok(Self::from_bytes(bytes.to_vec()))
    }
}

impl TableCodec for Vec<u8> {
    fn encode(&self) -> Vec<u8> {
        self.clone()
    }

    fn decode(bytes: &[u8]) -> StorageResult<Self> {
        Ok(bytes.to_vec())
    }
}
