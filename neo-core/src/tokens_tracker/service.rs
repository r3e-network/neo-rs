//! Tokens tracker service handle for RPC queries.

use crate::persistence::store::Store;
use std::sync::Arc;

use super::TokensTrackerSettings;

/// Lightweight service wrapper exposing tracker settings and store.
#[derive(Clone)]
pub struct TokensTrackerService {
    settings: TokensTrackerSettings,
    store: Arc<dyn Store>,
}

impl TokensTrackerService {
    /// Creates a service handle from tracker settings and the backing store.
    pub fn new(settings: TokensTrackerSettings, store: Arc<dyn Store>) -> Self {
        Self { settings, store }
    }

    /// Returns the tracker settings in effect.
    pub fn settings(&self) -> &TokensTrackerSettings {
        &self.settings
    }

    /// Returns a handle to the backing store.
    pub fn store(&self) -> Arc<dyn Store> {
        Arc::clone(&self.store)
    }
}
