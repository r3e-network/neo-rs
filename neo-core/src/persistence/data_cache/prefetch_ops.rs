use super::prefetch::PrefetchPattern;
use super::DataCache;
use crate::persistence::seek_direction::SeekDirection;
use crate::smart_contract::{StorageItem, StorageKey};
use std::sync::atomic::Ordering;
use tracing::trace;

impl DataCache {
    /// Records an access pattern for intelligent prefetching.
    pub(super) fn record_access_pattern(&self, key: &StorageKey) -> PrefetchPattern {
        let seq = self.access_seq.fetch_add(1, Ordering::Relaxed);
        self.pattern_tracker.write().record_access(key, seq)
    }

    /// Gets the current detected prefetch pattern.
    pub fn current_prefetch_pattern(&self) -> PrefetchPattern {
        self.pattern_tracker.read().current_pattern(30)
    }

    /// Checks if a key is in the prefetch window (recently prefetched).
    pub fn is_recently_prefetched(&self, key: &StorageKey) -> bool {
        self.prefetch_window.read().contains(key)
    }

    /// Clears the prefetch window.
    pub fn clear_prefetch_window(&self) {
        self.prefetch_window.write().clear();
    }

    /// Trigger prefetching based on detected access pattern.
    pub(super) fn trigger_prefetch_if_needed(&self, key: &StorageKey, pattern: PrefetchPattern) {
        if !self.config.enable_prefetching {
            return;
        }

        match pattern {
            PrefetchPattern::SequentialForward => {
                // Prefetch next keys in sequence
                self.prefetch_next_keys(key, self.config.prefetch_count);
            }
            PrefetchPattern::SequentialBackward => {
                // Prefetch previous keys in sequence
                self.prefetch_prev_keys(key, self.config.prefetch_count);
            }
            _ => {}
        }
    }

    /// Prefetch next sequential keys.
    fn prefetch_next_keys(&self, key: &StorageKey, count: usize) {
        if let Some(ref store_find) = self.store_find {
            let items: Vec<(StorageKey, StorageItem)> =
                store_find(Some(key), SeekDirection::Forward)
                    .into_iter()
                    .filter(|(k, _)| !self.is_recently_prefetched(k))
                    .take(count)
                    .collect();

            if !items.is_empty() {
                // Mark these as prefetched
                {
                    let mut window = self.prefetch_window.write();
                    for (k, _) in &items {
                        window.insert(k.clone());
                    }
                    // Limit window size
                    if window.len() > 1000 {
                        window.clear(); // Simple eviction: clear when too large
                    }
                }

                // Prefetch into read cache
                self.prefetch(items);
                trace!(target: "neo", count, "prefetched forward sequential keys");
            }
        }
    }

    /// Prefetch previous sequential keys.
    fn prefetch_prev_keys(&self, key: &StorageKey, count: usize) {
        if let Some(ref store_find) = self.store_find {
            let items: Vec<(StorageKey, StorageItem)> =
                store_find(Some(key), SeekDirection::Backward)
                    .into_iter()
                    .filter(|(k, _)| !self.is_recently_prefetched(k))
                    .take(count)
                    .collect();

            if !items.is_empty() {
                // Mark these as prefetched
                {
                    let mut window = self.prefetch_window.write();
                    for (k, _) in &items {
                        window.insert(k.clone());
                    }
                    if window.len() > 1000 {
                        window.clear();
                    }
                }

                self.prefetch(items);
                trace!(target: "neo", count, "prefetched backward sequential keys");
            }
        }
    }

    /// Pre-fetches items into the read cache.
    pub fn prefetch(&self, items: Vec<(StorageKey, StorageItem)>) {
        if let Some(ref cache) = self.read_cache {
            let cache_items: Vec<_> = items
                .into_iter()
                .map(|(k, v)| {
                    let size = v.value_bytes().len() + std::mem::size_of::<StorageKey>();
                    (k, v, size)
                })
                .collect();
            cache.put_batch(cache_items);
        }
    }
}
