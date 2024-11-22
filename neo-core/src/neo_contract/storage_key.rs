use std::mem::size_of;
use crate::cryptography::Murmur32;

/// Represents the keys in contract storage.
#[derive(Clone, Debug)]
pub struct StorageKey {
    /// The id of the contract.
    pub id: i32,

    /// The key of the storage entry.
    pub key: Vec<u8>,

    cache: Option<Vec<u8>>,
}

impl StorageKey {
    pub fn new() -> Self {
        StorageKey {
            id: 0,
            key: Vec::new(),
            cache: None,
        }
    }

    fn from_cache(cache: Vec<u8>) -> Self {
        let id = i32::from_le_bytes(cache[0..4].try_into().unwrap());
        let key = cache[4..].to_vec();
        StorageKey {
            id,
            key,
            cache: Some(cache),
        }
    }

    /// Creates a search prefix for a contract.
    pub fn create_search_prefix(id: i32, prefix: &[u8]) -> Vec<u8> {
        let mut buffer = Vec::with_capacity(size_of::<i32>() + prefix.len());
        buffer.extend_from_slice(&id.to_le_bytes());
        buffer.extend_from_slice(prefix);
        buffer
    }

    pub fn to_array(&mut self) -> Vec<u8> {
        if self.cache.is_none() {
            let mut cache = Vec::with_capacity(size_of::<i32>() + self.key.len());
            cache.extend_from_slice(&self.id.to_le_bytes());
            cache.extend_from_slice(&self.key);
            self.cache = Some(cache);
        }
        self.cache.as_ref().unwrap().clone()
    }
}

impl PartialEq for StorageKey {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.key == other.key
    }
}

impl Eq for StorageKey {}

impl std::hash::Hash for StorageKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let hash_value = self.id as u32 + Murmur32::hash(&self.key, 0);
        state.write_u32(hash_value);
    }
}

impl From<Vec<u8>> for StorageKey {
    fn from(value: Vec<u8>) -> Self {
        StorageKey::from_cache(value)
    }
}

impl From<&[u8]> for StorageKey {
    fn from(value: &[u8]) -> Self {
        StorageKey::from_cache(value.to_vec())
    }
}
