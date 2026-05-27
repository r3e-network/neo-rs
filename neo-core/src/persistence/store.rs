// Copyright (C) 2015-2025 The Neo Project.
//
// store.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::{
    read_only_store::ReadOnlyStore, store_snapshot::StoreSnapshot, write_store::WriteStore,
};
use std::any::Any;
use std::sync::Arc;

/// Delegate for OnNewSnapshot event
pub type OnNewSnapshotDelegate = Box<dyn Fn(&dyn Store, Arc<dyn StoreSnapshot>) + Send + Sync>;

/// This interface provides methods for reading, writing from/to database.
/// Developers should implement this interface to provide new storage engines for NEO.
pub trait Store: ReadOnlyStore + WriteStore<Vec<u8>, Vec<u8>> + Send + Sync + Any {
    /// Creates a snapshot of the database.
    fn snapshot(&self) -> Arc<dyn StoreSnapshot>;

    /// Event raised when a new snapshot is created
    fn on_new_snapshot(&self, handler: OnNewSnapshotDelegate);

    /// Enables storage-level fast-sync optimizations when supported.
    fn enable_fast_sync_mode(&self) {}

    /// Disables storage-level fast-sync optimizations.
    fn disable_fast_sync_mode(&self) {}

    /// Flushes pending writes to durable storage when supported.
    fn flush(&self) {}

    /// Downcast support for concrete implementations.
    fn as_any(&self) -> &dyn Any;
}
