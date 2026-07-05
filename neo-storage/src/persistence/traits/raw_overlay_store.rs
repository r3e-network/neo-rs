//! Raw overlay extension trait for [`Store`](super::store::Store).
//!
//! Backends that can consume materialized raw byte-key overlays implement
//! [`RawOverlayStore`]; backends that can't simply leave the
//! [`Store::as_raw_overlay_store`] accessor returning `None`.
//!
//! This trait was extracted from the monolithic `Store` trait in ADR-020 to
//! reduce the `Store` surface. The old `supports_raw_overlay_commit()` boolean
//! is no longer needed — the trait's existence (via the accessor) IS the
//! capability check.

use crate::error::StorageResult;

/// Raw overlay extension for [`Store`](super::store::Store).
///
/// Backends that support direct overlay commit implement this trait and
/// override [`Store::as_raw_overlay_store`] to return `Some(self)`.
///
/// The two commit methods are tried in order: first the borrowed visitor
/// (zero-copy), then the materialized `Vec` fallback. Both return `Ok(false)`
/// when the backend cannot handle the overlay, signaling the caller to fall
/// back to snapshot-based commit.
pub trait RawOverlayStore: super::store::Store {
    /// Commits raw byte-key overlay entries directly when the backend can do
    /// so without constructing a mutable snapshot. Backends that do not
    /// support a direct overlay commit should return `Ok(false)` so callers
    /// can fall back to snapshot-based commit.
    ///
    /// Implementations may sort this materialized overlay by raw key before
    /// writing so B+tree and LSM backends receive locality-friendly batches.
    fn try_commit_raw_overlay(&self, overlay: &[(Vec<u8>, Option<Vec<u8>>)])
    -> StorageResult<bool>;

    /// Commits raw byte-key overlay entries from a borrowed visitor when the
    /// backend can consume the changes without the caller first cloning them
    /// into a `Vec`.
    ///
    /// Callers should visit entries in raw byte-key order. `StoreCache`
    /// satisfies this contract through `DataCache::visit_raw_changes`, keeping
    /// the hot commit path sorted without forcing every backend to clone the
    /// overlay just to sort it again.
    ///
    /// Implementations should return `Ok(false)` when unsupported so callers
    /// can fall back to [`RawOverlayStore::try_commit_raw_overlay`] or
    /// snapshots.
    fn try_commit_borrowed_raw_overlay(
        &self,
        visit: &mut dyn FnMut(&mut dyn FnMut(&[u8], Option<&[u8]>)),
    ) -> StorageResult<bool>;
}
