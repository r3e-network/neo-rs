//! Tokens tracker service handle for RPC queries.

use neo_storage::persistence::providers::MemoryStore;
use neo_storage::persistence::store::Store;
use std::sync::Arc;

use super::TokensTrackerSettings;

/// Lightweight service wrapper exposing tracker settings and store.
#[derive(Clone)]
pub struct TokensTrackerService<S: Store = MemoryStore> {
    settings: TokensTrackerSettings,
    store: Arc<S>,
}

impl<S> TokensTrackerService<S>
where
    S: Store,
{
    /// Create a token tracker service over the given storage backend.
    pub fn new(settings: TokensTrackerSettings, store: Arc<S>) -> Self {
        Self { settings, store }
    }

    /// Return the token tracker configuration.
    pub fn settings(&self) -> &TokensTrackerSettings {
        &self.settings
    }

    /// Return the storage backend used by token tracker queries.
    pub fn store(&self) -> Arc<S> {
        Arc::clone(&self.store)
    }
}
