//! Neo Plugins Collection
//!
//! This crate provides a collection of plugins that extend Neo's functionality,
//! matching the C# Neo implementation plugins.

pub mod complete_plugin_system;
pub mod dbft_plugin;
pub mod rpc_server;
pub mod rest_server;
pub mod sqlite_wallet;
pub mod state_service;
pub mod tokens_tracker;

// Core plugin interfaces moved from neo-core
pub mod i_plugin_settings;
pub mod plugin;
pub mod unhandled_exception_policy;

// Re-export common types
pub use neo_extensions::plugin::{Plugin, PluginCategory, PluginContext, PluginEvent, PluginInfo};

/// Plugin collection for easy registration
pub struct PluginCollection;

impl PluginCollection {
    /// Get all available plugins (matches C# Neo plugin collection exactly)
    pub fn all_plugins() -> Vec<Box<dyn Plugin>> {
        vec![
            Box::new(dbft_plugin::DbftPlugin::new()),
            Box::new(rpc_server::RpcServerPlugin::new()),
            Box::new(rest_server::RestServerPlugin::new()),
            Box::new(state_service::StateServicePlugin::new()),
            Box::new(sqlite_wallet::SqliteWalletPlugin::new()),
            Box::new(tokens_tracker::TokensTrackerPlugin::new()),
        ]
    }

    /// Get core plugins (essential for most deployments)
    pub fn core_plugins() -> Vec<Box<dyn Plugin>> {
        vec![
            Box::new(dbft_plugin::DbftPlugin::new()),
            Box::new(rpc_server::RpcServerPlugin::new()),
            Box::new(tokens_tracker::TokensTrackerPlugin::new()),
        ]
    }

    /// Get RPC-related plugins
    pub fn rpc_plugins() -> Vec<Box<dyn Plugin>> {
        vec![
            Box::new(rpc_server::RpcServerPlugin::new()),
            Box::new(rest_server::RestServerPlugin::new()),
            Box::new(state_service::StateServicePlugin::new()),
            Box::new(tokens_tracker::TokensTrackerPlugin::new()),
        ]
    }

    /// Get utility plugins
    pub fn utility_plugins() -> Vec<Box<dyn Plugin>> {
        vec![
            Box::new(sqlite_wallet::SqliteWalletPlugin::new()),
        ]
    }
}
