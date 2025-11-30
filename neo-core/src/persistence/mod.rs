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
pub mod seek_direction;
pub mod serialization;
pub mod storage;
pub mod storage_item;
pub mod storage_key;
pub mod store_cache;
pub mod store_factory;
pub mod track_state;
pub mod transaction;

pub use data_cache::{DataCache, Trackable};
pub use i_read_only_store::{IReadOnlyStore, IReadOnlyStoreGeneric};
pub use i_store::IStore;
pub use i_store_provider::IStoreProvider;
pub use i_store_snapshot::IStoreSnapshot;
pub use i_write_store::IWriteStore;
pub use seek_direction::SeekDirection;
pub use storage::StorageConfig;
pub use storage_item::StorageItem;
pub use storage_key::StorageKey;
pub use store_cache::StoreCache;
pub use store_factory::StoreFactory;
pub use track_state::TrackState;
pub use transaction::StoreTransaction;
