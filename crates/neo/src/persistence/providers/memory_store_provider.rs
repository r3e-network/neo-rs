// Copyright (C) 2015-2025 The Neo Project.
//
// memory_store_provider.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::memory_store::MemoryStore;
use crate::persistence::{i_store::IStore, i_store_provider::IStoreProvider};
use std::sync::Arc;

/// A provider for creating MemoryStore instances.
pub struct MemoryStoreProvider;

impl MemoryStoreProvider {
    /// Creates a new MemoryStoreProvider.
    pub fn new() -> Self {
        Self
    }
}

impl Default for MemoryStoreProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl IStoreProvider for MemoryStoreProvider {
    fn name(&self) -> &str {
        "Memory"
    }

    fn get_store(&self, _path: &str) -> Arc<dyn IStore> {
        Arc::new(MemoryStore::new())
    }
}
