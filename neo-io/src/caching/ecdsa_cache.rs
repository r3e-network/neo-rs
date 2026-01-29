//! `ECDsaCache` - aligns with C# Neo.IO.Caching.ECDsaCache

use super::fifo_cache::FIFOCache;
use std::hash::Hash;
use std::ops::{Deref, DerefMut};

/// Cache item storing an elliptic-curve key and its associated signer instance.
#[derive(Clone)]
pub struct ECDsaCacheItem<TPoint, TSigner>
where
    TPoint: Clone,
    TSigner: Clone,
{
    /// Cached point used as the key.
    pub key: TPoint,
    /// Cached signer corresponding to the point.
    pub value: TSigner,
}

impl<TPoint, TSigner> ECDsaCacheItem<TPoint, TSigner>
where
    TPoint: Clone,
    TSigner: Clone,
{
    /// Creates a new cache item with the provided key/value pair.
    pub const fn new(key: TPoint, value: TSigner) -> Self {
        Self { key, value }
    }
}

/// FIFO cache specialised for ECDSA signer instances keyed by elliptic-curve points.
pub struct ECDsaCache<TPoint, TSigner>
where
    TPoint: Eq + Hash + Clone,
    TSigner: Clone,
{
    inner: FIFOCache<TPoint, ECDsaCacheItem<TPoint, TSigner>>,
}

impl<TPoint, TSigner> ECDsaCache<TPoint, TSigner>
where
    TPoint: Eq + Hash + Clone,
    TSigner: Clone,
{
    /// Default maximum number of cached signer instances, matching the C# constant (`20_000` entries).
    pub const DEFAULT_CAPACITY: usize = 20_000;

    /// Creates a cache with the specified maximum capacity.
    #[must_use] 
    pub fn new(max_capacity: usize) -> Self {
        Self {
            inner: FIFOCache::new(max_capacity, |item: &ECDsaCacheItem<TPoint, TSigner>| {
                item.key.clone()
            }),
        }
    }
}

impl<TPoint, TSigner> Default for ECDsaCache<TPoint, TSigner>
where
    TPoint: Eq + Hash + Clone,
    TSigner: Clone,
{
    fn default() -> Self {
        Self::new(Self::DEFAULT_CAPACITY)
    }
}

impl<TPoint, TSigner> Deref for ECDsaCache<TPoint, TSigner>
where
    TPoint: Eq + Hash + Clone,
    TSigner: Clone,
{
    type Target = FIFOCache<TPoint, ECDsaCacheItem<TPoint, TSigner>>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<TPoint, TSigner> DerefMut for ECDsaCache<TPoint, TSigner>
where
    TPoint: Eq + Hash + Clone,
    TSigner: Clone,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
