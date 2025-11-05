use std::collections::HashMap;

use dashmap::DashMap;

use crate::error::StoreError;
use crate::traits::{BatchOp, ColumnId, Store, WriteBatch};

#[derive(Default)]
pub struct MemoryStore {
    columns: DashMap<ColumnId, DashMap<Vec<u8>, Vec<u8>>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self {
            columns: DashMap::new(),
        }
    }

    pub fn with_columns(columns: &[ColumnId]) -> Self {
        let store = Self::new();
        for column in columns {
            store.create_column(*column);
        }
        store
    }

    pub fn create_column(&self, column: ColumnId) {
        self.columns.entry(column).or_insert_with(DashMap::new);
    }

    pub fn snapshot(&self) -> MemorySnapshot {
        let mut copy = HashMap::new();
        for column in self.columns.iter() {
            let mut map = HashMap::new();
            for kv in column.value().iter() {
                map.insert(kv.key().clone(), kv.value().clone());
            }
            copy.insert(*column.key(), map);
        }

        MemorySnapshot { columns: copy }
    }
}

impl Store for MemoryStore {
    fn get(&self, column: ColumnId, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        if let Some(col) = self.columns.get(&column) {
            Ok(col.value().get(key).map(|value| value.value().clone()))
        } else {
            Ok(None)
        }
    }

    fn put(&self, column: ColumnId, key: Vec<u8>, value: Vec<u8>) -> Result<(), StoreError> {
        self.columns
            .entry(column)
            .or_insert_with(DashMap::new)
            .value()
            .insert(key, value);
        Ok(())
    }

    fn delete(&self, column: ColumnId, key: &[u8]) -> Result<(), StoreError> {
        if let Some(col) = self.columns.get(&column) {
            col.value().remove(key);
        }
        Ok(())
    }

    fn write_batch(&self, batch: WriteBatch) -> Result<(), StoreError> {
        for op in batch.into_ops() {
            match op {
                BatchOp::Put { column, key, value } => {
                    self.put(column, key, value)?;
                }
                BatchOp::Delete { column, key } => {
                    self.delete(column, key.as_slice())?;
                }
            }
        }

        Ok(())
    }
}

#[derive(Clone, Default)]
pub struct MemorySnapshot {
    columns: HashMap<ColumnId, HashMap<Vec<u8>, Vec<u8>>>,
}

impl MemorySnapshot {
    pub fn get(&self, column: ColumnId, key: &[u8]) -> Option<&[u8]> {
        self.columns
            .get(&column)
            .and_then(|map| map.get(key))
            .map(|v| v.as_slice())
    }

    pub fn len(&self, column: ColumnId) -> usize {
        self.columns.get(&column).map(|m| m.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::columns::{BlockRecord, Blocks, HeaderRecord, Headers};
    use crate::traits::{Column, StoreExt};
    use neo_base::{hash::Hash256, Bytes, NeoDecode, NeoEncode, SliceReader};
    use serde_json::Value;

    const STATE: ColumnId = ColumnId::new("state");

    #[test]
    fn put_get_roundtrip() {
        let store = MemoryStore::with_columns(&[Headers::ID, Blocks::ID]);
        let header = HeaderRecord {
            hash: Hash256::new([1u8; 32]),
            height: 42,
            raw: Bytes::from(vec![0xAA, 0xBB, 0xCC]),
        };

        store
            .put_encoded(Headers::ID, &header.key(), &header)
            .unwrap();
        let fetched: HeaderRecord = store
            .get_decoded(Headers::ID, &header.key())
            .unwrap()
            .unwrap();
        assert_eq!(fetched, header);
    }

    #[test]
    fn batch_application() {
        let store = MemoryStore::new();
        let mut batch = WriteBatch::new();
        let block_one = BlockRecord {
            hash: Hash256::new([2u8; 32]),
            raw: Bytes::from(vec![1, 2, 3]),
        };
        let block_two = BlockRecord {
            hash: Hash256::new([3u8; 32]),
            raw: Bytes::from(vec![4, 5, 6]),
        };
        let key_one = encode_key(&block_one.key());
        let key_two = encode_key(&block_two.key());
        batch.put(
            Blocks::ID,
            key_one.clone(),
            block_one.raw.clone().into_vec(),
        );
        batch.put(
            Blocks::ID,
            key_two.clone(),
            block_two.raw.clone().into_vec(),
        );
        batch.delete(Blocks::ID, key_one.clone());

        store.write_batch(batch).unwrap();
        assert!(store.get(Blocks::ID, key_one.as_slice()).unwrap().is_none());
        let stored = store.get(Blocks::ID, key_two.as_slice()).unwrap().unwrap();
        assert_eq!(stored, block_two.raw.clone().into_vec());
    }

    #[test]
    fn snapshot_freeze() {
        let store = MemoryStore::new();
        store.put(STATE, b"k".to_vec(), b"v1".to_vec()).unwrap();
        let snapshot = store.snapshot();

        store.put(STATE, b"k".to_vec(), b"v2".to_vec()).unwrap();

        let snap_val = snapshot.get(STATE, b"k").unwrap();
        assert_eq!(snap_val, b"v1");
        let current = store.get(STATE, b"k").unwrap().unwrap();
        assert_eq!(current, b"v2");
    }
    #[test]
    fn typed_helpers() {
        let store = MemoryStore::new();
        store.put_encoded::<u32, u64>(STATE, &7, &99).unwrap();
        let value = store.get_decoded::<u32, u64>(STATE, &7).unwrap();
        assert_eq!(value, Some(99));
        store.delete_encoded::<u32>(STATE, &7).unwrap();
        assert!(store.get_decoded::<u32, u64>(STATE, &7).unwrap().is_none());
    }

    #[test]
    fn header_fixture_roundtrip() {
        let json = include_str!("../fixtures/header.json");
        let record: HeaderRecord = serde_json::from_str(json).unwrap();

        let mut buf = Vec::new();
        record.neo_encode(&mut buf);
        let mut reader = SliceReader::new(buf.as_slice());
        let decoded = HeaderRecord::neo_decode(&mut reader).unwrap();
        assert_eq!(decoded, record);

        let emitted = serde_json::to_string(&record).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(json).unwrap(),
            serde_json::from_str::<Value>(&emitted).unwrap()
        );
    }

    #[test]
    fn block_fixture_roundtrip() {
        let json = include_str!("../fixtures/block.json");
        let record: BlockRecord = serde_json::from_str(json).unwrap();

        let mut buf = Vec::new();
        record.neo_encode(&mut buf);
        let mut reader = SliceReader::new(buf.as_slice());
        let decoded = BlockRecord::neo_decode(&mut reader).unwrap();
        assert_eq!(decoded, record);
    }

    fn encode_key<T: NeoEncode>(value: &T) -> Vec<u8> {
        let mut buf = Vec::new();
        value.neo_encode(&mut buf);
        buf
    }
}
