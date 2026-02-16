// Copyright (C) 2015-2025 The Neo Project.
//
// store_factory.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::{
    i_store::IStore, i_store_provider::IStoreProvider,
    providers::memory_store_provider::MemoryStoreProvider,
};
use crate::error::{CoreError, CoreResult};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

/// Global registry of store providers.
static PROVIDERS: LazyLock<RwLock<HashMap<String, Arc<dyn IStoreProvider>>>> =
    LazyLock::new(|| {
        let mut providers = HashMap::new();

        // Register default memory provider
        let mem_provider = Arc::new(MemoryStoreProvider::new()) as Arc<dyn IStoreProvider>;
        providers.insert("Memory".to_string(), mem_provider.clone());
        providers.insert("".to_string(), mem_provider); // Default case

        RwLock::new(providers)
    });

/// Factory for creating stores.
pub struct StoreFactory;

impl StoreFactory {
    /// Register a store provider.
    pub fn register_provider(provider: Arc<dyn IStoreProvider>) {
        let mut providers = PROVIDERS.write();
        providers.insert(provider.name().to_string(), provider);
    }

    /// Get store provider by name.
    pub fn get_store_provider(name: &str) -> Option<Arc<dyn IStoreProvider>> {
        let providers = PROVIDERS.read();
        providers.get(name).cloned()
    }

    /// Get store from name.
    ///
    /// # Arguments
    /// * `storage_provider` - The storage engine used to create the IStore objects.
    ///   If this parameter is empty, a default in-memory storage engine will be used.
    /// * `path` - The path of the storage.
    ///   If storage_provider is the default in-memory storage engine, this parameter is ignored.
    pub fn get_store(storage_provider: &str, path: &str) -> CoreResult<Arc<dyn IStore>> {
        let providers = PROVIDERS.read();
        let provider = providers
            .get(storage_provider)
            .or_else(|| providers.get(""))
            .cloned()
            .ok_or_else(|| CoreError::invalid_operation("Store provider not found"))?;
        provider.get_store(path)
    }
}
