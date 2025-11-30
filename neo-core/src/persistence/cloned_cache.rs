//! Mirrors `Neo.Persistence.ClonedCache` from the C# implementation.

use super::data_cache::{DataCache, DataCacheBackend};

/// Lightweight wrapper that provides a writable clone of an existing [`DataCache`].
///
/// The original cache remains untouched; changes must be committed explicitly back to the
/// underlying store by consumers if desired.
#[derive(Debug, Clone)]
pub struct ClonedCache<B: DataCacheBackend> {
    inner: DataCache<B>,
}

impl<B: DataCacheBackend> ClonedCache<B> {
    /// Creates a cloned cache from an existing [`DataCache`].
    pub fn new(cache: &DataCache<B>) -> Self {
        Self {
            inner: cache.clone(),
        }
    }

    /// Borrows the cloned cache mutably.
    pub fn cache(&mut self) -> &mut DataCache<B> {
        &mut self.inner
    }

    /// Consumes the wrapper and returns the inner cache.
    pub fn into_inner(self) -> DataCache<B> {
        self.inner
    }
}
