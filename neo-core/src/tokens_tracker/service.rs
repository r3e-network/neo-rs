//! Tokens tracker service handle for RPC queries.

use crate::persistence::i_store::IStore;
use std::sync::Arc;

use super::TokensTrackerSettings;

/// Lightweight service wrapper exposing tracker settings and store.
#[derive(Clone)]
pub struct TokensTrackerService {
    settings: TokensTrackerSettings,
    store: Arc<dyn IStore>,
}

impl TokensTrackerService {
    pub fn new(settings: TokensTrackerSettings, store: Arc<dyn IStore>) -> Self {
        Self { settings, store }
    }

    pub fn settings(&self) -> &TokensTrackerSettings {
        &self.settings
    }

    pub fn store(&self) -> Arc<dyn IStore> {
        Arc::clone(&self.store)
    }
}
