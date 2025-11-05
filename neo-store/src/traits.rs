use std::fmt;

use neo_base::encoding::{NeoDecode, NeoEncode, SliceReader};

use crate::error::StoreError;

/// Named identifier for a column family.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ColumnId(pub &'static str);

impl ColumnId {
    pub const fn new(name: &'static str) -> Self {
        Self(name)
    }

    #[inline]
    pub const fn name(self) -> &'static str {
        self.0
    }
}

impl From<&'static str> for ColumnId {
    #[inline]
    fn from(value: &'static str) -> Self {
        ColumnId::new(value)
    }
}

impl fmt::Display for ColumnId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// Marker trait linking a type to a column identifier.
pub trait Column {
    const ID: ColumnId;
}

/// Operation to be applied via a write batch.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum BatchOp {
    Put {
        column: ColumnId,
        key: Vec<u8>,
        value: Vec<u8>,
    },
    Delete {
        column: ColumnId,
        key: Vec<u8>,
    },
}

/// Ordered set of operations that should be applied atomically.
#[derive(Debug, Default, Clone)]
pub struct WriteBatch {
    ops: Vec<BatchOp>,
}

impl WriteBatch {
    #[inline]
    pub fn new() -> Self {
        Self { ops: Vec::new() }
    }

    #[inline]
    pub fn put(&mut self, column: ColumnId, key: Vec<u8>, value: Vec<u8>) {
        self.ops.push(BatchOp::Put { column, key, value });
    }

    #[inline]
    pub fn delete(&mut self, column: ColumnId, key: Vec<u8>) {
        self.ops.push(BatchOp::Delete { column, key });
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    #[inline]
    pub fn operations(&self) -> &[BatchOp] {
        &self.ops
    }

    #[inline]
    pub fn into_ops(self) -> Vec<BatchOp> {
        self.ops
    }
}

/// Abstraction exposed by storage backends.
pub trait Store: Send + Sync {
    fn get(&self, column: ColumnId, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError>;

    fn put(&self, column: ColumnId, key: Vec<u8>, value: Vec<u8>) -> Result<(), StoreError>;

    fn delete(&self, column: ColumnId, key: &[u8]) -> Result<(), StoreError>;

    fn write_batch(&self, batch: WriteBatch) -> Result<(), StoreError>;
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
