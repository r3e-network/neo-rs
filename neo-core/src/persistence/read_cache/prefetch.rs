use super::{BloomFilterKey, ReadCache};
use std::hash::Hash;
use std::sync::Arc;

/// Pre-fetch hint for sequential access patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefetchHint {
    /// No pre-fetching.
    None,
    /// Pre-fetch forward (next keys).
    Forward,
    /// Pre-fetch backward (previous keys).
    Backward,
    /// Pre-fetch both directions.
    Both,
}

/// Iterator with pre-fetching support.
pub struct PrefetchingIterator<K, V, I, F>
where
    K: Clone + Eq + Hash + BloomFilterKey,
    V: Clone,
    I: Iterator<Item = (K, V)>,
    F: Fn(&K) -> Vec<(K, V)>,
{
    inner: I,
    prefetch_fn: F,
    cache: Arc<ReadCache<K, V>>,
    hint: PrefetchHint,
    buffer: Vec<(K, V)>,
    buffer_pos: usize,
}

impl<K, V, I, F> PrefetchingIterator<K, V, I, F>
where
    K: Clone + Eq + Hash + BloomFilterKey,
    V: Clone,
    I: Iterator<Item = (K, V)>,
    F: Fn(&K) -> Vec<(K, V)>,
{
    /// Creates a new pre-fetching iterator.
    pub fn new(inner: I, cache: Arc<ReadCache<K, V>>, prefetch_fn: F, hint: PrefetchHint) -> Self {
        Self {
            inner,
            prefetch_fn,
            cache,
            hint,
            buffer: Vec::new(),
            buffer_pos: 0,
        }
    }

    /// Pre-fetches items based on the current key.
    fn prefetch(&mut self, key: &K) {
        if self.hint == PrefetchHint::None {
            return;
        }

        let items = (self.prefetch_fn)(key);

        if !items.is_empty() {
            let cache_items: Vec<_> = items
                .into_iter()
                .map(|(k, v)| {
                    let size = std::mem::size_of_val(&k) + std::mem::size_of_val(&v);
                    (k, v, size)
                })
                .collect();

            self.cache.put_batch(cache_items);
        }
    }
}

impl<K, V, I, F> Iterator for PrefetchingIterator<K, V, I, F>
where
    K: Clone + Eq + Hash + BloomFilterKey,
    V: Clone,
    I: Iterator<Item = (K, V)>,
    F: Fn(&K) -> Vec<(K, V)>,
{
    type Item = (K, V);

    fn next(&mut self) -> Option<Self::Item> {
        // Return from buffer first
        if self.buffer_pos < self.buffer.len() {
            let item = self.buffer.get(self.buffer_pos).cloned();
            self.buffer_pos += 1;
            return item;
        }

        // Get next item from inner iterator
        if let Some((key, value)) = self.inner.next() {
            // Trigger pre-fetch
            self.prefetch(&key);

            Some((key, value))
        } else {
            None
        }
    }
}
