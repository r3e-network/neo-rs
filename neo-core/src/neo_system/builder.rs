//! Builder pattern for NeoSystem construction.
//!
//! This module provides a fluent builder interface for creating `NeoSystem` instances
//! with customizable configuration options.

use std::sync::Arc;

use crate::error::CoreResult;
use crate::persistence::i_store_provider::IStoreProvider;
use crate::protocol_settings::ProtocolSettings;

use super::NeoSystem;

/// Builder for constructing a `NeoSystem` instance.
///
/// # Example
///
/// ```ignore
/// use neo_core::neo_system::NeoSystemBuilder;
/// use neo_core::protocol_settings::ProtocolSettings;
///
/// let system = NeoSystemBuilder::new()
///     .with_settings(ProtocolSettings::mainnet())
///     .with_storage_path("/data/neo")
///     .build()?;
/// ```
#[derive(Default)]
pub struct NeoSystemBuilder {
    settings: Option<ProtocolSettings>,
    storage_provider: Option<Arc<dyn IStoreProvider>>,
    storage_path: Option<String>,
}

impl NeoSystemBuilder {
    /// Creates a new builder with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the protocol settings for the system.
    ///
    /// If not specified, defaults to mainnet settings.
    pub fn with_settings(mut self, settings: ProtocolSettings) -> Self {
        self.settings = Some(settings);
        self
    }

    /// Sets the storage provider for persistence.
    ///
    /// If not specified, defaults to the memory store provider.
    pub fn with_storage_provider(mut self, provider: Arc<dyn IStoreProvider>) -> Self {
        self.storage_provider = Some(provider);
        self
    }

    /// Sets the storage path for the database.
    ///
    /// This is typically a directory path where RocksDB or other
    /// persistent storage will write data.
    pub fn with_storage_path(mut self, path: impl Into<String>) -> Self {
        self.storage_path = Some(path.into());
        self
    }

    /// Configures the builder for mainnet operation.
    pub fn mainnet(mut self) -> Self {
        self.settings = Some(ProtocolSettings::mainnet());
        self
    }

    /// Configures the builder for testnet operation.
    pub fn testnet(mut self) -> Self {
        self.settings = Some(ProtocolSettings::testnet());
        self
    }

    /// Builds the `NeoSystem` instance.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The actor system fails to initialize
    /// - The storage provider fails to create a store
    /// - Plugin initialization fails
    pub fn build(self) -> CoreResult<Arc<NeoSystem>> {
        let settings = self.settings.unwrap_or_else(ProtocolSettings::mainnet);
        NeoSystem::new(settings, self.storage_provider, self.storage_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_creates_with_defaults() {
        // Just verify the builder can be constructed
        let builder = NeoSystemBuilder::new();
        assert!(builder.settings.is_none());
        assert!(builder.storage_provider.is_none());
        assert!(builder.storage_path.is_none());
    }

    #[test]
    fn builder_chain_methods() {
        let builder = NeoSystemBuilder::new()
            .mainnet()
            .with_storage_path("/tmp/neo-test");

        assert!(builder.settings.is_some());
        assert_eq!(builder.storage_path, Some("/tmp/neo-test".to_string()));
    }

    #[test]
    fn builder_testnet_sets_settings() {
        let builder = NeoSystemBuilder::new().testnet();
        assert!(builder.settings.is_some());
        // Testnet has different network magic
        assert_eq!(builder.settings.unwrap().network, 0x3554334E);
    }
}
