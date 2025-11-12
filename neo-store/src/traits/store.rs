use neo_base::encoding::{NeoDecode, NeoEncode, SliceReader};

use crate::error::StoreError;

use super::{ColumnId, WriteBatch};

/// Abstraction exposed by storage backends.
pub trait Store: Send + Sync {
    fn get(&self, column: ColumnId, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError>;

    fn put(&self, column: ColumnId, key: Vec<u8>, value: Vec<u8>) -> Result<(), StoreError>;

    fn delete(&self, column: ColumnId, key: &[u8]) -> Result<(), StoreError>;

    fn write_batch(&self, batch: WriteBatch) -> Result<(), StoreError>;

    fn scan_prefix(
        &self,
        column: ColumnId,
        prefix: &[u8],
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>, StoreError>;
}

/// Convenience helpers for working with strongly typed keys and values.
pub trait StoreExt: Store {
    fn put_encoded<K: NeoEncode, V: NeoEncode>(
        &self,
        column: ColumnId,
        key: &K,
        value: &V,
    ) -> Result<(), StoreError> {
        self.put(column, key.to_vec(), value.to_vec())
    }

    fn get_decoded<K: NeoEncode, V: NeoDecode>(
        &self,
        column: ColumnId,
        key: &K,
    ) -> Result<Option<V>, StoreError> {
        let key_bytes = key.to_vec();
        match self.get(column, key_bytes.as_slice())? {
            Some(bytes) => {
                let mut reader = SliceReader::new(bytes.as_slice());
                V::neo_decode(&mut reader)
                    .map(Some)
                    .map_err(|err| StoreError::backend(format!("decode error: {err}")))
            }
            None => Ok(None),
        }
    }

    fn delete_encoded<K: NeoEncode>(&self, column: ColumnId, key: &K) -> Result<(), StoreError> {
        let key_bytes = key.to_vec();
        self.delete(column, key_bytes.as_slice())
    }
}

impl<T: Store + ?Sized> StoreExt for T {}
