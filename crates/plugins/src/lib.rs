//! Neo Plugins Collection
//!
//! This crate provides a collection of plugins that extend Neo's functionality,
//! matching the C# Neo implementation plugins.

pub mod application_logs;
pub mod dbft_plugin;
pub mod leveldb_store;
pub mod oracle_service;
pub mod rest_server;
pub mod rocksdb_store;
pub mod rpc_server;
pub mod sign_client;
pub mod sqlite_wallet;
pub mod state_service;
pub mod storage_dumper;
pub mod tokens_tracker;

// Re-export common types
pub use neo_extensions::plugin::{Plugin, PluginCategory, PluginContext, PluginEvent, PluginInfo};

/// Plugin collection for easy registration
pub struct PluginCollection;

impl PluginCollection {
    /// Get all available plugins (matches C# Neo plugin collection exactly)
    pub fn all_plugins() -> Vec<Box<dyn Plugin>> {
        vec![
            Box::new(application_logs::ApplicationLogsPlugin::new(application_logs::ApplicationLogsSettings::default())),
            Box::new(dbft_plugin::DBFTPlugin::new(dbft_plugin::DbftSettings::default())),
            Box::new(leveldb_store::LevelDBStore::new(leveldb_store::LevelDBStoreSettings::default())),
            Box::new(oracle_service::OracleServicePlugin::new(oracle_service::OracleServiceSettings::default())),
            Box::new(rest_server::RestServerPlugin::new()),
            Box::new(rocksdb_store::RocksDBStore::new()),
            Box::new(rpc_server::RpcServerPlugin::new()),
            Box::new(sign_client::SignClient::new(sign_client::SignClientSettings::default())),
            Box::new(sqlite_wallet::SqliteWalletPlugin::new()),
            Box::new(state_service::StateServicePlugin::new()),
            Box::new(storage_dumper::StorageDumper::new(storage_dumper::StorageDumperSettings::default())),
            Box::new(tokens_tracker::TokensTrackerPlugin::new()),
        ]
    }

    /// Get core plugins (essential for most deployments)
    pub fn core_plugins() -> Vec<Box<dyn Plugin>> {
        vec![
            Box::new(dbft_plugin::DBFTPlugin::new(dbft_plugin::DbftSettings::default())),
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
