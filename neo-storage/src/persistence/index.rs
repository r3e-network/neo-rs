use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;

/// Index configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexConfig {
    pub name: String,
    pub index_type: IndexType,
    pub unique: bool,
    pub case_sensitive: bool,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            index_type: IndexType::BTree,
            unique: false,
            case_sensitive: true,
        }
    }
}

/// Index type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexType {
    BTree,
    Hash,
    Composite,
}

/// Index statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexStats {
    pub entries: usize,
    pub lookups: u64,
    pub inserts: u64,
    pub deletes: u64,
    pub memory_usage: usize,
}

/// BTree index implementation
pub struct BTreeIndex<K, V>
where
    K: Clone + Ord,
    V: Clone,
{
    name: String,
    data: BTreeMap<K, Vec<V>>,
    #[allow(dead_code)]
    config: IndexConfig,
    stats: IndexStats,
    unique: bool,
}

impl<K, V> BTreeIndex<K, V>
where
    K: Clone + Ord,
    V: Clone + PartialEq,
{
    pub fn new(name: String) -> Self {
        Self {
            name: name.clone(),
            data: BTreeMap::new(),
            config: IndexConfig {
                name,
                index_type: IndexType::BTree,
                unique: false,
                case_sensitive: true,
            },
            stats: IndexStats::default(),
            unique: false,
        }
    }

    pub fn with_config(config: IndexConfig) -> Self {
        Self {
            name: config.name.clone(),
            data: BTreeMap::new(),
            unique: config.unique,
            config,
            stats: IndexStats::default(),
        }
    }

    pub fn insert(&mut self, key: K, value: V) -> Result<(), String> {
        if self.unique {
            if self.data.contains_key(&key) {
                return Err(format!("Duplicate key in unique index: {:?}", self.name));
            }
            self.data.insert(key, vec![value]);
        } else {
            self.data.entry(key).or_default().push(value);
        }
        self.stats.inserts += 1;
        self.stats.entries = self.data.len();
        self.update_memory_usage();
        Ok(())
    }

    pub fn get(&mut self, key: &K) -> Option<Vec<V>> {
        self.stats.lookups += 1;
        self.data.get(key).cloned()
    }

    pub fn range(&mut self, start: &K, end: &K) -> Vec<(K, Vec<V>)> {
        self.stats.lookups += 1;
        self.data
            .range(start..=end)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    pub fn remove(&mut self, key: &K, value: Option<&V>) -> bool {
        let removed = if let Some(values) = self.data.get_mut(key) {
            if let Some(target_value) = value {
                if let Some(pos) = values.iter().position(|v| v == target_value) {
                    values.remove(pos);
                    if values.is_empty() {
                        self.data.remove(key);
                    }
                    true
                } else {
                    false
                }
            } else {
                self.data.remove(key);
                true
            }
        } else {
            false
        };

        if removed {
            self.stats.deletes += 1;
            self.stats.entries = self.data.len();
            self.update_memory_usage();
        }
        removed
    }

    pub fn clear(&mut self) {
        self.data.clear();
        self.stats.entries = 0;
        self.update_memory_usage();
    }

    pub fn stats(&self) -> &IndexStats {
        &self.stats
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    fn update_memory_usage(&mut self) {
        let key_size = std::mem::size_of::<K>();
        let value_size = std::mem::size_of::<V>();
        let vec_overhead = std::mem::size_of::<Vec<V>>();
        let mut total_size = 0;
        for values in self.data.values() {
            total_size += key_size + vec_overhead + (values.len() * value_size);
        }
        self.stats.memory_usage = total_size;
    }
}

/// Hash index implementation
pub struct HashIndex<K, V>
where
    K: Clone + Eq + Hash,
    V: Clone,
{
    name: String,
    data: HashMap<K, Vec<V>>,
    #[allow(dead_code)]
    config: IndexConfig,
    stats: IndexStats,
    unique: bool,
}

impl<K, V> HashIndex<K, V>
where
    K: Clone + Eq + Hash,
    V: Clone + PartialEq,
{
    pub fn new(name: String) -> Self {
        Self {
            name: name.clone(),
            data: HashMap::new(),
            config: IndexConfig {
                name,
                index_type: IndexType::Hash,
                unique: false,
                case_sensitive: true,
            },
            stats: IndexStats::default(),
            unique: false,
        }
    }

    pub fn with_config(config: IndexConfig) -> Self {
        Self {
            name: config.name.clone(),
            data: HashMap::new(),
            unique: config.unique,
            config,
            stats: IndexStats::default(),
        }
    }

    pub fn insert(&mut self, key: K, value: V) -> Result<(), String> {
        if self.unique {
            if self.data.contains_key(&key) {
                return Err(format!("Duplicate key in unique index: {:?}", self.name));
            }
            self.data.insert(key, vec![value]);
        } else {
            self.data.entry(key).or_default().push(value);
        }
        self.stats.inserts += 1;
        self.stats.entries = self.data.len();
        self.update_memory_usage();
        Ok(())
    }

    pub fn get(&mut self, key: &K) -> Option<Vec<V>> {
        self.stats.lookups += 1;
        self.data.get(key).cloned()
    }

    pub fn remove(&mut self, key: &K, value: Option<&V>) -> bool {
        let removed = if let Some(values) = self.data.get_mut(key) {
            if let Some(target_value) = value {
                if let Some(pos) = values.iter().position(|v| v == target_value) {
                    values.remove(pos);
                    if values.is_empty() {
                        self.data.remove(key);
                    }
                    true
                } else {
                    false
                }
            } else {
                self.data.remove(key);
                true
            }
        } else {
            false
        };

        if removed {
            self.stats.deletes += 1;
            self.stats.entries = self.data.len();
            self.update_memory_usage();
        }
        removed
    }

    pub fn clear(&mut self) {
        self.data.clear();
        self.stats.entries = 0;
        self.update_memory_usage();
    }

    pub fn stats(&self) -> &IndexStats {
        &self.stats
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    fn update_memory_usage(&mut self) {
        let key_size = std::mem::size_of::<K>();
        let value_size = std::mem::size_of::<V>();
        let vec_overhead = std::mem::size_of::<Vec<V>>();
        let mut total_size = 0;
        for values in self.data.values() {
            total_size += key_size + vec_overhead + (values.len() * value_size);
        }
        self.stats.memory_usage = total_size;
    }
}
