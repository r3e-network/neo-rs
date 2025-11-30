//! RocksDB storage plugin shim - the default node already registers this backend.

use neo_core::extensions::plugin::{Plugin, PluginInfo};

/// Placeholder plugin retained for compatibility with the C# manifest.
pub struct RocksDBStorePlugin {
    info: PluginInfo,
}

impl RocksDBStorePlugin {
    pub fn new() -> Self {
        Self {
            info: PluginInfo {
                name: "RocksDBStore".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: "RocksDB storage backend (built into neo-core)".to_string(),
                author: "Neo Project".to_string(),
                dependencies: vec![],
                min_neo_version: "3.6.0".to_string(),
                category: neo_core::extensions::plugin::PluginCategory::Storage,
                priority: 0,
            },
        }
    }
}

impl Default for RocksDBStorePlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Plugin for RocksDBStorePlugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }

    async fn initialize(
        &mut self,
        _context: &neo_core::extensions::plugin::PluginContext,
    ) -> neo_core::extensions::error::ExtensionResult<()> {
        // No-op: neo-core already registers the RocksDB provider via configuration.
        Ok(())
    }

    async fn start(&mut self) -> neo_core::extensions::error::ExtensionResult<()> {
        Ok(())
    }

    async fn stop(&mut self) -> neo_core::extensions::error::ExtensionResult<()> {
        Ok(())
    }

    async fn handle_event(
        &mut self,
        _event: &neo_core::extensions::plugin::PluginEvent,
    ) -> neo_core::extensions::error::ExtensionResult<()> {
        Ok(())
    }
}
