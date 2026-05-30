//! Tokens tracker service handle for RPC queries.

use neo_core::persistence::store::Store;
use std::sync::Arc;

use super::TokensTrackerSettings;

/// Lightweight service wrapper exposing tracker settings and store.
#[derive(Clone)]
pub struct TokensTrackerService {
    settings: TokensTrackerSettings,
    store: Arc<dyn Store>,
}

impl TokensTrackerService {
    pub fn new(settings: TokensTrackerSettings, store: Arc<dyn Store>) -> Self {
        Self { settings, store }
    }

    pub fn settings(&self) -> &TokensTrackerSettings {
        &self.settings
    }

    pub fn store(&self) -> Arc<dyn Store> {
        Arc::clone(&self.store)
    }
}
