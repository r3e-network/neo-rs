// Copyright (C) 2015-2025 The Neo Project.
//
// mod.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

//! Persistence module for Neo blockchain
//!
//! **IMPORTANT**: This module provides **concrete storage implementations** for neo-core.
//! For abstract storage traits only (without neo-core dependency), use [`neo_storage`] crate.
//!
//! ## When to use this module
//!
//! - **Production nodes**: RocksDB-backed storage with full feature support
//! - **DataCache**: C# parity caching layer with track states and commit logic
//! - **Smart contract storage**: Integration with `StorageKey`/`StorageItem` types
//! - **Store providers**: `IStoreProvider` implementations with snapshot support
//!
//! ## When to use neo-storage
//!
//! - **Trait bounds**: When you need to accept any storage backend generically
//! - **No neo-core dependency**: For standalone tools that only need storage interfaces
//! - **Testing**: Mock implementations using the simple `IReadOnlyStore`/`IWriteStore` traits
//!
//! This module provides persistence functionality matching the C# Neo.Persistence namespace.

pub mod cache;
pub mod compression;
pub mod data_cache;
pub mod i_read_only_store;
pub mod i_store;
pub mod i_store_provider;
pub mod i_store_snapshot;
pub mod i_write_store;
pub mod index;
pub mod providers;
pub mod read_cache;
pub mod seek_direction;
pub mod serialization;
pub mod storage;
pub mod storage_item;
pub mod storage_key;
pub mod store_cache;
pub mod store_factory;
pub mod track_state;
pub mod transaction;
pub mod write_batch_buffer;

pub use data_cache::{DataCache, Trackable};
pub use i_read_only_store::{IReadOnlyStore, IReadOnlyStoreGeneric};
pub use i_store::IStore;
pub use i_store_provider::IStoreProvider;
pub use i_store_snapshot::IStoreSnapshot;
pub use i_write_store::IWriteStore;
pub use read_cache::{
    PrefetchHint, ReadCache, ReadCacheConfig, ReadCacheStats, ReadCacheStatsSnapshot,
    StorageReadCache,
};
pub use seek_direction::SeekDirection;
pub use storage::StorageConfig;
pub use storage_item::StorageItem;
pub use storage_key::StorageKey;
pub use store_cache::StoreCache;
pub use store_factory::StoreFactory;
pub use track_state::TrackState;
pub use transaction::StoreTransaction;
#[cfg(feature = "rocksdb")]
pub use write_batch_buffer::{AutoFlushBatchBuffer, WriteBatchBuffer};
pub use write_batch_buffer::{WriteBatchConfig, WriteBatchStats, WriteBatchStatsSnapshot};
