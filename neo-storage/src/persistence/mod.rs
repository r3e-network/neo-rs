//! Persistence abstractions shared by in-memory and RocksDB storage backends.

/// Data cache with Neo-style trackable entries.
pub mod data_cache;
/// Built-in store providers used by tests and ephemeral nodes.
pub mod providers;
/// Read-only store traits.
pub mod read_only_store;
/// Iteration direction for seek/find operations.
pub mod seek_direction;
pub mod storage;
/// Combined read/write store traits and snapshot callbacks.
pub mod store;
pub mod store_cache;
/// Factory abstraction for named store providers.
pub mod store_factory;
/// Store provider trait.
pub mod store_provider;
/// Mutable point-in-time store snapshots.
pub mod store_snapshot;
/// Track states used by cached storage entries.
pub mod track_state;
pub mod transaction;
/// Write-only store trait.
pub mod write_store;

pub use data_cache::{DataCache, Trackable};
pub use read_only_store::{ReadOnlyStore, ReadOnlyStoreGeneric};
pub use seek_direction::SeekDirection;
pub use store::Store;
pub use store_cache::StoreCache;
pub use store_factory::StoreFactory;
pub use store_provider::StoreProvider;
pub use store_snapshot::StoreSnapshot;
pub use track_state::TrackState;
pub use transaction::StoreTransaction;
pub use write_store::WriteStore;
