//! # neo-storage::persistence
//!
//! Persistence traits, snapshots, transactions, and cache overlays.
//!
//! ## Boundary
//!
//! This module belongs to `neo-storage`. This infrastructure crate owns store
//! mechanics and must not execute contracts, import blocks, or make RPC/network
//! policy decisions.
//!
//! ## Contents
//!
//! - `data_cache`: Write-back cache implementation and tracked-entry state.
//! - `providers`: Provider implementations behind the crate public traits.
//! - `fast_sync_store`: Fast-sync extension trait (ADR-021).
//! - `read_only_store`: read-only store trait.
//! - `raw_overlay_store`: Raw overlay extension trait (ADR-021).
//! - `seek_direction`: seek direction enum.
//! - `storage`: Storage contexts, key builders, and storage item helpers for
//!   execution.
//! - `store`: Core store trait (read + write + snapshot + downcast).
//! - `store_cache`: store-backed cache overlay.
//! - `store_factory`: store factory trait.
//! - `store_provider`: store provider trait.
//! - `store_snapshot`: snapshot store trait.
//! - `table`: Typed table metadata and byte-preserving table codecs.
//! - `track_state`: tracked mutation state enum.
//! - `transaction`: Transaction body, signer, witness, and fee records.
//! - `write_store`: write store trait.

/// Data cache with Neo-style trackable entries.
pub mod data_cache;
/// Built-in store providers used by tests and ephemeral nodes.
pub mod providers;
/// Fast-sync extension trait for [`Store`](store::Store).
#[path = "traits/fast_sync_store.rs"]
pub mod fast_sync_store;
/// Read-only store traits.
#[path = "traits/read_only_store.rs"]
pub mod read_only_store;
/// Raw overlay extension trait for [`Store`](store::Store).
#[path = "traits/raw_overlay_store.rs"]
pub mod raw_overlay_store;
/// Iteration direction for seek/find operations.
#[path = "traits/seek_direction.rs"]
pub mod seek_direction;
#[path = "cache/storage.rs"]
pub mod storage;
/// Combined read/write store traits and snapshot callbacks.
#[path = "traits/store.rs"]
pub mod store;
#[path = "cache/store_cache.rs"]
pub mod store_cache;
/// Factory abstraction for named store providers.
#[path = "traits/store_factory.rs"]
pub mod store_factory;
/// Store provider trait.
#[path = "traits/store_provider.rs"]
pub mod store_provider;
/// Mutable point-in-time store snapshots.
#[path = "traits/store_snapshot.rs"]
pub mod store_snapshot;
/// Typed table boundary over raw byte-key stores.
pub mod table;
/// Track states used by cached storage entries.
#[path = "cache/track_state.rs"]
pub mod track_state;
#[path = "transactions/transaction.rs"]
pub mod transaction;
/// Write-only store trait.
#[path = "traits/write_store.rs"]
pub mod write_store;

pub use data_cache::{DataCache, Trackable};
pub use fast_sync_store::FastSyncStore;
pub use read_only_store::{RawReadOnlyStore, ReadOnlyStore, ReadOnlyStoreGeneric};
pub use raw_overlay_store::RawOverlayStore;
pub use seek_direction::SeekDirection;
pub use store::Store;
pub use store_cache::StoreCache;
pub use store_factory::StoreFactory;
pub use store_provider::StoreProvider;
pub use store_snapshot::StoreSnapshot;
pub use table::{StoreTableRead, Table, TableCodec, TableReader};
pub use track_state::TrackState;
pub use transaction::StoreTransaction;
pub use write_store::WriteStore;

#[cfg(test)]
#[path = "../tests/persistence/table.rs"]
mod table_tests;
