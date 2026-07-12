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
//! - `read_only_store`: read-only store trait.
//! - `seek_direction`: seek direction enum.
//! - `storage`: Storage contexts, key builders, and storage item helpers for
//!   execution.
//! - `store`: Core store trait (read + write + snapshot + backend capabilities).
//! - `store_cache`: store-backed cache overlay.
//! - `store_factory`: named provider registry and store factory facade.
//! - `store_maintenance`: isolated node metadata and atomic maintenance batches.
//! - `store_provider`: backend provider trait implemented by concrete stores.
//! - `store_snapshot`: snapshot store trait.
//! - `table`: Statically dispatched logical tables and byte codecs.
//! - `transactional_store`: Mandatory atomic commit capabilities for node stores.
//! - `track_state`: tracked mutation state enum.
//! - `transaction`: Transaction body, signer, witness, and fee records.
//! - `write_store`: write store trait.

/// Data cache with Neo-style trackable entries.
pub mod data_cache;
/// Built-in store providers used by tests and ephemeral nodes.
pub mod providers;
/// Read-only store traits.
#[path = "traits/read_only_store.rs"]
pub mod read_only_store;
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
/// Registry-backed factory facade for named store providers.
#[path = "traits/store_factory.rs"]
pub mod store_factory;
/// Atomic data and node-local metadata maintenance operations.
#[path = "traits/store_maintenance.rs"]
pub mod store_maintenance;
/// Backend provider trait implemented by concrete store adapters.
#[path = "traits/store_provider.rs"]
pub mod store_provider;
/// Mutable point-in-time store snapshots.
#[path = "traits/store_snapshot.rs"]
pub mod store_snapshot;
/// Typed logical-table definitions, codecs, and provider reads.
pub mod table;
/// Track states used by cached storage entries.
#[path = "cache/track_state.rs"]
pub mod track_state;
#[path = "transactions/transaction.rs"]
pub mod transaction;
/// Atomic canonical and maintenance transaction capabilities.
#[path = "traits/transactional_store.rs"]
pub mod transactional_store;
/// Write-only store trait.
#[path = "traits/write_store.rs"]
pub mod write_store;

pub use data_cache::{CacheRead, DataCache, EmptyCacheBacking, Trackable};
pub use read_only_store::{RawReadOnlyStore, ReadOnlyStore, ReadOnlyStoreGeneric};
pub use seek_direction::SeekDirection;
pub use store::{
    MdbxEnvironmentInfo, RawOverlaySink, RawOverlaySource, RocksDbBatchMetrics, Store,
    StoreBackendKind,
};
pub use store_cache::{StoreCache, StoreCacheBacking, StoreDataCache};
pub use store_factory::StoreFactory;
pub use store_maintenance::StoreMaintenanceBatch;
pub use store_provider::StoreProvider;
pub use store_snapshot::StoreSnapshot;
pub use table::{
    BytesCodec, FixedBytesCodec, IntoTableBytes, StorageItemCodec, StorageKeyCodec, Table,
    TableCodec, TableDecode, TableEncode, TableNamespace, TableProvider, U32BeCodec, U64BeCodec,
};
pub use track_state::TrackState;
pub use transaction::StoreTransaction;
pub use transactional_store::TransactionalStore;
pub use write_store::WriteStore;
