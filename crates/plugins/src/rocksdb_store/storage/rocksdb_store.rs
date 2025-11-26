use super::store::RocksDBStoreProvider;
use async_trait::async_trait;
use neo_extensions::error::ExtensionResult;
use neo_extensions::plugin::{Plugin, PluginCategory, PluginContext, PluginEvent, PluginInfo};
use std::sync::Once;

static REGISTER_PROVIDER: Once = Once::new();

fn ensure_provider_registered() {
    REGISTER_PROVIDER.call_once(|| {
        RocksDBStoreProvider::register();
    });
}

/// RocksDBStore plugin mirroring Neo.Plugins.Storage.RocksDBStore.
pub struct RocksDBStore {
    info: PluginInfo,
}

impl RocksDBStore {
    pub fn new() -> Self {
        ensure_provider_registered();

        Self {
            info: PluginInfo {
                name: "RocksDBStore".to_string(),
                version: "1.0.0".to_string(),
                description: "Uses RocksDBStore to store the blockchain data".to_string(),
                author: "Neo Project".to_string(),
                dependencies: vec![],
                min_neo_version: "3.6.0".to_string(),
                category: PluginCategory::Storage,
                priority: 0,
            },
        }
    }
}

impl Default for RocksDBStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for RocksDBStore {
    fn info(&self) -> &PluginInfo {
        &self.info
    }

    async fn initialize(&mut self, _context: &PluginContext) -> ExtensionResult<()> {
        ensure_provider_registered();
        Ok(())
    }

    async fn start(&mut self) -> ExtensionResult<()> {
        Ok(())
    }

    async fn stop(&mut self) -> ExtensionResult<()> {
        Ok(())
    }

    async fn handle_event(&mut self, _event: &PluginEvent) -> ExtensionResult<()> {
        Ok(())
    }
}
