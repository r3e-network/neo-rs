//! Mandatory atomic transaction capabilities for canonical node stores.
//!
//! `Store` describes general read/write/snapshot behavior and optional
//! throughput fast paths. This trait is the stronger composition contract for
//! stores that can publish a canonical overlay atomically and keep node-local
//! maintenance metadata isolated in the same transaction as data mutations.

use super::store::{RawOverlaySource, Store};
use super::store_maintenance::StoreMaintenanceBatch;
use crate::StorageResult;

/// Store capability required by canonical node and durable sidecar workflows.
///
/// Persistent implementations must not return from either commit method until
/// the transaction has crossed their durability fence. The in-memory provider
/// implements the same atomic visibility contract for tests and explicitly
/// ephemeral nodes, but naturally does not survive process restart.
pub trait TransactionalStore: Store {
    /// Atomically publishes one canonical raw overlay.
    ///
    /// The source visits entries in raw key order. Implementations must either
    /// publish every operation or publish none of them.
    fn commit_canonical_overlay<O>(&self, overlay: &mut O) -> StorageResult<()>
    where
        O: RawOverlaySource + ?Sized;

    /// Reads one value from the isolated node-maintenance namespace.
    fn maintenance_metadata(&self, key: &[u8]) -> StorageResult<Option<Vec<u8>>>;

    /// Atomically commits normal data mutations and isolated maintenance
    /// metadata mutations as one transaction.
    fn commit_maintenance(&self, batch: &StoreMaintenanceBatch) -> StorageResult<()>;
}
