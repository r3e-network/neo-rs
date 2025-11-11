use std::collections::HashMap;

use crate::traits::ColumnId;

#[derive(Clone, Default)]
pub struct MemorySnapshot {
    columns: HashMap<ColumnId, HashMap<Vec<u8>, Vec<u8>>>,
}

impl MemorySnapshot {
    pub(crate) fn from_dashmap(
        columns: &dashmap::DashMap<ColumnId, dashmap::DashMap<Vec<u8>, Vec<u8>>>,
    ) -> Self {
        let mut copy = HashMap::new();
        for column in columns.iter() {
            let mut map = HashMap::new();
            for kv in column.value().iter() {
                map.insert(kv.key().clone(), kv.value().clone());
            }
            copy.insert(*column.key(), map);
        }
        MemorySnapshot { columns: copy }
    }

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
