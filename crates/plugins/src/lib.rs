//! Neo Plugins Collection
//!
//! This crate provides a collection of plugins that extend Neo's functionality,
//! matching the C# Neo implementation plugins.

pub mod application_logs;
pub mod dbft_plugin;
pub mod rocksdb_store;
pub mod rpc_server;
// Rest server, sign client, storage dumper, oracle, and state service are removed from this port.
pub mod sqlite_wallet;
pub mod tokens_tracker;
// Experimental/unsupported modules have been removed from the Rust port for now.

// Re-export common types
pub use neo_extensions::plugin::{Plugin, PluginCategory, PluginContext, PluginEvent, PluginInfo};

/// Plugin collection for easy registration
pub struct PluginCollection;

impl PluginCollection {
    /// Get all available plugins (matches C# Neo plugin collection exactly)
    pub fn all_plugins() -> Vec<Box<dyn Plugin>> {
        vec![
            Box::new(application_logs::ApplicationLogsPlugin::new()),
            Box::new(dbft_plugin::DBFTPlugin::new()),
            Box::new(rocksdb_store::RocksDBStore::new()),
            Box::new(RpcServerPlugin::new()),
            Box::new(sqlite_wallet::SqliteWalletPlugin::new()),
            Box::new(tokens_tracker::TokensTrackerPlugin::new()),
        ]
    }

    /// Get core plugins (essential for most deployments)
    pub fn core_plugins() -> Vec<Box<dyn Plugin>> {
        vec![
            Box::new(dbft_plugin::DBFTPlugin::new()),
            Box::new(RpcServerPlugin::new()),
            Box::new(tokens_tracker::TokensTrackerPlugin::new()),
        ]
    }

    /// Get RPC-related plugins
    pub fn rpc_plugins() -> Vec<Box<dyn Plugin>> {
        vec![
            Box::new(RpcServerPlugin::new()),
            Box::new(tokens_tracker::TokensTrackerPlugin::new()),
        ]
    }

    /// Get utility plugins
    pub fn utility_plugins() -> Vec<Box<dyn Plugin>> {
        vec![Box::new(sqlite_wallet::SqliteWalletPlugin::new())]
    }
}
use crate::rpc_server::rpc_server_plugin::RpcServerPlugin;
