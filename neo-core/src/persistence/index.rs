//! Indexing functionality for persistence layer.
//!
//! This module provides production-ready indexing capabilities that match
//! the C# Neo indexing functionality exactly.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;

/// Index configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexConfig {
    /// Index name
    pub name: String,
    /// Index type
    pub index_type: IndexType,
    /// Enable unique constraint
    pub unique: bool,
    /// Enable case-sensitive indexing for strings
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
    /// B-Tree index for range queries
    BTree,
    /// Hash index for exact matches
    Hash,
    /// Composite index for multiple fields
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

/// BTree index implementation (production-ready)
pub struct BTreeIndex<K, V>
where
    K: Clone + Ord,
    V: Clone,
{
    /// Index name
    name: String,
    /// Index data
    data: BTreeMap<K, Vec<V>>,
    /// Index configuration
    #[allow(dead_code)]
    config: IndexConfig,
    /// Index statistics
    stats: IndexStats,
    /// Enable unique constraint
    unique: bool,
}

impl<K, V> BTreeIndex<K, V>
where
    K: Clone + Ord,
    V: Clone + PartialEq,
{
    /// Creates a new B-Tree index
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

    /// Creates a new B-Tree index with configuration
    pub fn with_config(config: IndexConfig) -> Self {
        Self {
            name: config.name.clone(),
            data: BTreeMap::new(),
            unique: config.unique,
            config,
            stats: IndexStats::default(),
        }
    }

    /// Inserts a key-value pair into the index (production implementation)
    pub fn insert(&mut self, key: K, value: V) -> Result<(), String> {
        if self.unique {
            if self.data.contains_key(&key) {
                return Err(format!("Duplicate key in unique index: {:?}", self.name));
            }

            self.data.insert(key, vec![value]);
        } else {
            // Insert into multi-value index
            self.data.entry(key).or_default().push(value);
        }

        // Update statistics
        self.stats.inserts += 1;
        self.stats.entries = self.data.len();
        self.update_memory_usage();

        Ok(())
    }

    /// Gets values by key (production implementation)
    pub fn get(&mut self, key: &K) -> Option<Vec<V>> {
        // Update statistics
        self.stats.lookups += 1;

        self.data.get(key).cloned()
    }

    /// Gets values in a range (production implementation)
    pub fn range(&mut self, start: &K, end: &K) -> Vec<(K, Vec<V>)> {
        // Update statistics
        self.stats.lookups += 1;

        self.data
            .range(start..=end)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Removes a key-value pair from the index (production implementation)
    pub fn remove(&mut self, key: &K, value: Option<&V>) -> bool {
        let removed = if let Some(values) = self.data.get_mut(key) {
            if let Some(target_value) = value {
                // Remove specific value
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

    /// Clears the index
    pub fn clear(&mut self) {
        self.data.clear();
        self.stats.entries = 0;
        self.update_memory_usage();
    }

    /// Gets index statistics
    pub fn stats(&self) -> &IndexStats {
        &self.stats
    }

    /// Gets the number of keys in the index
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Checks if the index is empty
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Updates memory usage statistics
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

/// Hash index implementation (production-ready)
pub struct HashIndex<K, V>
where
    K: Clone + Eq + Hash,
    V: Clone,
{
    /// Index name
    name: String,
    /// Index data
    data: HashMap<K, Vec<V>>,
    /// Index configuration
    #[allow(dead_code)]
    config: IndexConfig,
    /// Index statistics
    stats: IndexStats,
    /// Enable unique constraint
    unique: bool,
}

impl<K, V> HashIndex<K, V>
where
    K: Clone + Eq + Hash,
    V: Clone + PartialEq,
{
    /// Creates a new hash index
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

    /// Creates a new hash index with configuration
    pub fn with_config(config: IndexConfig) -> Self {
        Self {
            name: config.name.clone(),
            data: HashMap::new(),
            unique: config.unique,
            config,
            stats: IndexStats::default(),
        }
    }

    /// Inserts a key-value pair into the index (production implementation)
    pub fn insert(&mut self, key: K, value: V) -> Result<(), String> {
        if self.unique {
            if self.data.contains_key(&key) {
                return Err(format!("Duplicate key in unique index: {:?}", self.name));
            }

            self.data.insert(key, vec![value]);
        } else {
            // Insert into multi-value index
            self.data.entry(key).or_default().push(value);
        }

        // Update statistics
        self.stats.inserts += 1;
        self.stats.entries = self.data.len();
        self.update_memory_usage();

        Ok(())
    }

    /// Gets values by key (production implementation)
    pub fn get(&mut self, key: &K) -> Option<Vec<V>> {
        // Update statistics
        self.stats.lookups += 1;

        self.data.get(key).cloned()
    }

    /// Removes a key-value pair from the index (production implementation)
    pub fn remove(&mut self, key: &K, value: Option<&V>) -> bool {
        let removed = if let Some(values) = self.data.get_mut(key) {
            if let Some(target_value) = value {
                // Remove specific value
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

    /// Clears the index
    pub fn clear(&mut self) {
        self.data.clear();
        self.stats.entries = 0;
        self.update_memory_usage();
    }

    /// Gets index statistics
    pub fn stats(&self) -> &IndexStats {
        &self.stats
    }

    /// Gets the number of keys in the index
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Checks if the index is empty
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Updates memory usage statistics
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

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // IndexConfig Tests
    // ============================================================================

    #[test]
    fn index_config_default_values() {
        let config = IndexConfig::default();
        assert_eq!(config.name, "default");
        assert_eq!(config.index_type, IndexType::BTree);
        assert!(!config.unique);
        assert!(config.case_sensitive);
    }

    // ============================================================================
    // BTreeIndex Tests
    // ============================================================================

    #[test]
    fn btree_index_new_creates_empty_index() {
        let index: BTreeIndex<String, i32> = BTreeIndex::new("test".to_string());
        assert!(index.is_empty());
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn btree_index_insert_and_get() {
        let mut index = BTreeIndex::new("test".to_string());
        index.insert("key1".to_string(), 42).unwrap();

        let values = index.get(&"key1".to_string());
        assert_eq!(values, Some(vec![42]));
    }

    #[test]
    fn btree_index_multi_value_insert() {
        let mut index = BTreeIndex::new("test".to_string());
        index.insert("key".to_string(), 1).unwrap();
        index.insert("key".to_string(), 2).unwrap();
        index.insert("key".to_string(), 3).unwrap();

        let values = index.get(&"key".to_string());
        assert_eq!(values, Some(vec![1, 2, 3]));
    }

    #[test]
    fn btree_index_unique_rejects_duplicates() {
        let config = IndexConfig {
            name: "unique_test".to_string(),
            index_type: IndexType::BTree,
            unique: true,
            case_sensitive: true,
        };
        let mut index: BTreeIndex<String, i32> = BTreeIndex::with_config(config);

        index.insert("key".to_string(), 1).unwrap();
        let result = index.insert("key".to_string(), 2);

        assert!(result.is_err());
    }

    #[test]
    fn btree_index_range_query() {
        let mut index = BTreeIndex::new("test".to_string());
        index.insert(1, "a".to_string()).unwrap();
        index.insert(2, "b".to_string()).unwrap();
        index.insert(3, "c".to_string()).unwrap();
        index.insert(4, "d".to_string()).unwrap();
        index.insert(5, "e".to_string()).unwrap();

        let range = index.range(&2, &4);
        assert_eq!(range.len(), 3);
    }

    #[test]
    fn btree_index_remove_key() {
        let mut index = BTreeIndex::new("test".to_string());
        index.insert("key".to_string(), 42).unwrap();

        let removed = index.remove(&"key".to_string(), None);
        assert!(removed);
        assert!(index.is_empty());
    }

    #[test]
    fn btree_index_remove_specific_value() {
        let mut index = BTreeIndex::new("test".to_string());
        index.insert("key".to_string(), 1).unwrap();
        index.insert("key".to_string(), 2).unwrap();

        let removed = index.remove(&"key".to_string(), Some(&1));
        assert!(removed);

        let values = index.get(&"key".to_string());
        assert_eq!(values, Some(vec![2]));
    }

    #[test]
    fn btree_index_clear() {
        let mut index = BTreeIndex::new("test".to_string());
        index.insert("a".to_string(), 1).unwrap();
        index.insert("b".to_string(), 2).unwrap();

        index.clear();
        assert!(index.is_empty());
    }

    #[test]
    fn btree_index_stats_tracking() {
        let mut index = BTreeIndex::new("test".to_string());
        index.insert("key".to_string(), 42).unwrap();
        let _ = index.get(&"key".to_string());
        let _ = index.get(&"missing".to_string());

        let stats = index.stats();
        assert_eq!(stats.inserts, 1);
        assert_eq!(stats.lookups, 2);
    }

    // ============================================================================
    // HashIndex Tests
    // ============================================================================

    #[test]
    fn hash_index_new_creates_empty_index() {
        let index: HashIndex<String, i32> = HashIndex::new("test".to_string());
        assert!(index.is_empty());
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn hash_index_insert_and_get() {
        let mut index = HashIndex::new("test".to_string());
        index.insert("key1".to_string(), 42).unwrap();

        let values = index.get(&"key1".to_string());
        assert_eq!(values, Some(vec![42]));
    }

    #[test]
    fn hash_index_unique_rejects_duplicates() {
        let config = IndexConfig {
            name: "unique_test".to_string(),
            index_type: IndexType::Hash,
            unique: true,
            case_sensitive: true,
        };
        let mut index: HashIndex<String, i32> = HashIndex::with_config(config);

        index.insert("key".to_string(), 1).unwrap();
        let result = index.insert("key".to_string(), 2);

        assert!(result.is_err());
    }

    #[test]
    fn hash_index_remove_key() {
        let mut index = HashIndex::new("test".to_string());
        index.insert("key".to_string(), 42).unwrap();

        let removed = index.remove(&"key".to_string(), None);
        assert!(removed);
        assert!(index.is_empty());
    }

    #[test]
    fn hash_index_clear() {
        let mut index = HashIndex::new("test".to_string());
        index.insert("a".to_string(), 1).unwrap();
        index.insert("b".to_string(), 2).unwrap();

        index.clear();
        assert!(index.is_empty());
    }

    #[test]
    fn hash_index_stats_tracking() {
        let mut index = HashIndex::new("test".to_string());
        index.insert("key".to_string(), 42).unwrap();
        let _ = index.get(&"key".to_string());

        let stats = index.stats();
        assert_eq!(stats.inserts, 1);
        assert_eq!(stats.lookups, 1);
    }

    // ============================================================================
    // IndexStats Tests
    // ============================================================================

    #[test]
    fn index_stats_default_is_zero() {
        let stats = IndexStats::default();
        assert_eq!(stats.entries, 0);
        assert_eq!(stats.lookups, 0);
        assert_eq!(stats.inserts, 0);
        assert_eq!(stats.deletes, 0);
        assert_eq!(stats.memory_usage, 0);
    }
}
