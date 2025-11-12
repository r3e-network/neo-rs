use std::path::Path;

use std::collections::HashMap;

use sled::{Batch, Db};

use crate::{
    error::StoreError,
    traits::{BatchOp, ColumnId, Store, WriteBatch},
};

/// Persistent store backed by the `sled` embedded database.
pub struct SledStore {
    db: Db,
}

impl SledStore {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, StoreError> {
        let db = sled::open(path).map_err(|err| StoreError::backend(err.to_string()))?;
        Ok(Self { db })
    }

    fn column_tree(&self, column: ColumnId) -> Result<sled::Tree, StoreError> {
        self.db
            .open_tree(column.name())
            .map_err(|err| StoreError::backend(err.to_string()))
    }
}

impl Store for SledStore {
    fn get(&self, column: ColumnId, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        let tree = self.column_tree(column)?;
        tree.get(key)
            .map_err(|err| StoreError::backend(err.to_string()))
            .map(|opt| opt.map(|ivec| ivec.as_ref().to_vec()))
    }

    fn put(&self, column: ColumnId, key: Vec<u8>, value: Vec<u8>) -> Result<(), StoreError> {
        let tree = self.column_tree(column)?;
        tree.insert(key, value)
            .map_err(|err| StoreError::backend(err.to_string()))?;
        Ok(())
    }

    fn delete(&self, column: ColumnId, key: &[u8]) -> Result<(), StoreError> {
        let tree = self.column_tree(column)?;
        tree.remove(key)
            .map_err(|err| StoreError::backend(err.to_string()))?;
        Ok(())
    }

    fn write_batch(&self, batch: WriteBatch) -> Result<(), StoreError> {
        let mut trees: HashMap<ColumnId, Batch> = HashMap::new();
        for op in batch.into_ops() {
            match op {
                BatchOp::Put { column, key, value } => {
                    trees
                        .entry(column)
                        .or_insert_with(Batch::default)
                        .insert(key, value);
                }
                BatchOp::Delete { column, key } => {
                    trees
                        .entry(column)
                        .or_insert_with(Batch::default)
                        .remove(key);
                }
            }
        }

        for (column, batch) in trees {
            let tree = self.column_tree(column)?;
            tree.apply_batch(batch)
                .map_err(|err| StoreError::backend(err.to_string()))?;
        }
        self.db
            .flush()
            .map_err(|err| StoreError::backend(err.to_string()))?;
        Ok(())
    }

    fn scan_prefix(
        &self,
        column: ColumnId,
        prefix: &[u8],
    ) -> Result<Vec<(Vec<u8>, Vec<u8>)>, StoreError> {
        let tree = self.column_tree(column)?;
        let mut entries = Vec::new();
        for item in tree
            .scan_prefix(prefix)
            .map_err(|err| StoreError::backend(err.to_string()))?
        {
            let (key, value) = item.map_err(|err| StoreError::backend(err.to_string()))?;
            entries.push((key.as_ref().to_vec(), value.as_ref().to_vec()));
        }
        Ok(entries)
    }
}
