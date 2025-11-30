//! Stub plugin for SQLite wallets.
//!
//! This mirrors the surface area of the C# SQLite wallet plugin but does no
//! runtime work. It exists to keep the optional feature compiling cleanly.

use async_trait::async_trait;
use neo_core::extensions::error::ExtensionResult;
use neo_core::extensions::plugin::{Plugin, PluginBase, PluginCategory, PluginContext, PluginEvent, PluginInfo};

pub struct SqliteWalletPlugin {
    base: PluginBase,
}

impl SqliteWalletPlugin {
    pub fn new() -> Self {
        let info = PluginInfo {
            name: "SQLiteWallet".to_string(),
            version: "0.0.1".to_string(),
            description: "SQLite wallet plugin (not yet supported in neo-rs)".to_string(),
            author: "neo-rs".to_string(),
            dependencies: Vec::new(),
            min_neo_version: "4.0.0".to_string(),
            category: PluginCategory::Wallet,
            priority: 0,
        };
        Self {
            base: PluginBase::new(info),
        }
    }
}

impl Default for SqliteWalletPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Plugin for SqliteWalletPlugin {
    fn info(&self) -> &PluginInfo {
        self.base.info()
    }

    async fn initialize(&mut self, _context: &PluginContext) -> ExtensionResult<()> {
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
