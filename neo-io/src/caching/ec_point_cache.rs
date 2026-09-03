//! `ECPointCache` - aligns with C# Neo.IO.Caching.ECPointCache

use super::fifo_cache::FIFOCache;
use std::hash::Hash;

/// Trait representing an elliptic-curve point that can be encoded to a compressed byte array.
pub trait EncodablePoint: Clone + Send + Sync + 'static {
    /// Encodes the point to a compressed byte representation, matching C# `ECPoint.EncodePoint(true)`.
    fn encode_point_compressed(&self) -> Vec<u8>;
}

/// FIFO cache specialised for elliptic-curve points, mirroring C# `ECPointCache` behaviour.
pub struct ECPointCache<TPoint>
where
    TPoint: EncodablePoint + Eq + Hash,
{
    inner: FIFOCache<Vec<u8>, TPoint>,
}

impl<TPoint> ECPointCache<TPoint>
where
    TPoint: EncodablePoint + Eq + Hash,
{
    /// Creates a new cache with the provided maximum capacity (same semantics as C# constructor).
    #[must_use]
    pub fn new(max_capacity: usize) -> Self {
        Self {
            inner: FIFOCache::new(max_capacity, |point: &TPoint| {
                point.encode_point_compressed()
            }),
        }
    }
}

impl_cache_wrapper_deref! {
    impl<TPoint> for ECPointCache<TPoint>
    where {
        TPoint: EncodablePoint + Eq + Hash,
    }
    => FIFOCache<Vec<u8>, TPoint>
}
