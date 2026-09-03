use super::StateStore;
use crate::state_service::root_cache::StateRootCacheStats;
use crate::state_service::StateRoot;
use crate::UInt256;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, warn};

impl StateStore {
    /// Gets a state root from cache or storage.
    ///
    /// This method first checks the LRU cache, then falls back to disk.
    ///
    /// # Arguments
    /// * `index` - The block index
    ///
    /// # Returns
    /// The state root if found, None otherwise
    pub fn get_cached_state_root(&self, index: u32) -> Option<StateRoot> {
        // Check cache first
        if let Some(entry) = self.root_cache.write().get(index) {
            return Some(entry.state_root);
        }

        // Fall back to storage
        self.get_state_root(index)
    }

    /// Gets a state root by its hash from cache.
    ///
    /// # Arguments
    /// * `hash` - The state root hash
    ///
    /// # Returns
    /// The state root if found in cache, None otherwise
    pub fn get_cached_state_root_by_hash(&self, hash: &UInt256) -> Option<StateRoot> {
        self.root_cache
            .write()
            .get_by_hash(hash)
            .map(|e| e.state_root)
    }

    /// Caches a state root for future lookups.
    ///
    /// # Arguments
    /// * `state_root` - The state root to cache
    /// * `is_validated` - Whether this root has been consensus validated
    /// * `timestamp` - Optional timestamp (defaults to current time)
    pub fn cache_state_root(
        &self,
        state_root: StateRoot,
        is_validated: bool,
        timestamp: Option<u64>,
    ) {
        let ts = timestamp.unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        });

        self.root_cache
            .write()
            .insert_state_root(state_root, is_validated, ts);
    }

    /// Gets the root cache statistics.
    pub fn root_cache_stats(&self) -> Arc<StateRootCacheStats> {
        self.root_cache.read().stats()
    }

    /// Clears the root cache.
    pub fn clear_root_cache(&self) {
        self.root_cache.write().clear();
        debug!(target: "state", "state root cache cleared");
    }

    /// Gets the number of entries in the root cache.
    pub fn root_cache_len(&self) -> usize {
        self.root_cache.read().len()
    }

    /// Preloads recent state roots into the cache.
    ///
    /// This is useful during node startup to warm up the cache.
    ///
    /// # Arguments
    /// * `count` - Number of recent state roots to preload
    pub fn preload_recent_roots(&self, count: usize) {
        let Some(current_index) = self.local_root_index() else {
            return;
        };

        let start_index = current_index.saturating_sub(count as u32);
        for index in start_index..=current_index {
            if let Some(root) = self.get_state_root(index) {
                let is_validated = self.validated_root_index().is_some_and(|v| v >= index);
                self.root_cache.write().insert_state_root(
                    root,
                    is_validated,
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                );
            }
        }

        debug!(
            target: "state",
            preloaded = count.min((current_index - start_index + 1) as usize),
            "state root cache warmed up"
        );
    }

    /// Validates that the state root at the given index has been properly computed.
    ///
    /// This checks both the existence of the state root and optionally its witness
    /// if it should be a validated root.
    ///
    /// # Arguments
    /// * `index` - The block index to check
    /// * `require_validated` - Whether to require a validated (witnessed) root
    ///
    /// # Returns
    /// `true` if the state root is valid, `false` otherwise
    pub fn validate_state_root_exists(&self, index: u32, require_validated: bool) -> bool {
        match self.get_cached_state_root(index) {
            Some(root) => {
                if require_validated {
                    root.witness.is_some()
                } else {
                    true
                }
            }
            None => false,
        }
    }

    /// Compares local state root with a network-provided state root.
    ///
    /// This is used during synchronization to detect state inconsistencies.
    ///
    /// # Arguments
    /// * `index` - The block index
    /// * `network_root_hash` - The state root hash from the network
    ///
    /// # Returns
    /// `true` if local and network roots match, `false` otherwise
    pub fn compare_with_network_root(&self, index: u32, network_root_hash: &UInt256) -> bool {
        match self.get_cached_state_root(index) {
            Some(local_root) => &local_root.root_hash == network_root_hash,
            None => {
                warn!(
                    target: "state",
                    index,
                    "Cannot compare state root: local root not found"
                );
                false
            }
        }
    }
}
