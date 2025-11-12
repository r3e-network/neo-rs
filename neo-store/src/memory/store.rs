use dashmap::DashMap;

use super::snapshot::MemorySnapshot;
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
        MemorySnapshot::from_dashmap(&self.columns)
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

    fn scan_prefix(
        &self,
        column: ColumnId,
        prefix: &[u8],
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>, StoreError> {
        if let Some(col) = self.columns.get(&column) {
            let mut entries: Vec<(Vec<u8>, Vec<u8>)> = col
                .value()
                .iter()
                .filter_map(|kv| {
                    let key = kv.key();
                    if key.starts_with(prefix) {
                        Some((key.clone(), kv.value().clone()))
                    } else {
                        None
                    }
                })
                .collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Ok(entries)
        } else {
            Ok(Vec::new())
        }
    }
}
