//! Fast-sync extension trait for [`Store`](super::store::Store).
//!
//! Not all storage backends support fast-sync optimizations (WAL disabling,
//! auto-compaction throttling, buffered writes). Backends that do implement
//! [`FastSyncStore`]; backends that don't simply leave the
//! [`Store::as_fast_sync_store`] accessor returning `None`.
//!
//! This trait was extracted from the monolithic `Store` trait in ADR-020 to
//! reduce the `Store` surface from 19 methods / 6 concerns to 12 methods /
//! 4 concerns.

/// Fast-sync extension for [`Store`](super::store::Store).
///
/// Backends that support fast-sync mode implement this trait and override
/// [`Store::as_fast_sync_store`] to return `Some(self)`.
///
/// # Fast-sync lifecycle
///
/// 1. `enable_fast_sync_mode()` — called at startup during initial catch-up
/// 2. Normal writes proceed with reduced durability guarantees
/// 3. `disable_fast_sync_mode()` — called once catch-up is complete
/// 4. `flush()` — ensures all buffered writes reach durable storage
///
/// If the import aborts before completion, `discard_pending_fast_sync_writes()`
/// drops any buffered writes that haven't reached durable storage yet.
pub trait FastSyncStore: super::store::Store {
    /// Enables storage-level fast-sync optimizations (WAL disabled,
    /// auto-compaction off, buffered writes).
    fn enable_fast_sync_mode(&self);

    /// Disables storage-level fast-sync optimizations, restoring normal
    /// durability guarantees.
    fn disable_fast_sync_mode(&self);

    /// Drops pending fast-sync buffered writes that have not reached durable
    /// storage. Used only when an import aborts before its accepted prefix is
    /// finalized; successful imports must flush instead.
    fn discard_pending_fast_sync_writes(&self);

    /// Returns whether fast-sync writes have been accepted by the backend but
    /// are not guaranteed visible through fresh snapshots yet.
    fn has_pending_fast_sync_writes(&self) -> bool;
}
