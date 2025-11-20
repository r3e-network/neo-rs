//! Neo Plugins Collection
//!
//! This crate provides a collection of plugins that extend Neo's functionality,
//! matching the C# Neo implementation plugins.

#[cfg(feature = "application-logs")]
pub mod application_logs;
#[cfg(feature = "dbft")]
pub mod dbft_plugin;
#[cfg(feature = "leveldb-store")]
pub mod leveldb_store;
#[cfg(feature = "oracle")]
pub mod oracle_service;
#[cfg(feature = "rest-server")]
pub mod rest_server;
#[cfg(feature = "rocksdb-store")]
pub mod rocksdb_store;
#[cfg(any(feature = "rpc-server", test))]
pub mod rpc_server;
#[cfg(feature = "sign-client")]
pub mod sign_client;
#[cfg(feature = "sqlite-wallet")]
pub mod sqlite_wallet;
#[cfg(feature = "state-service")]
pub mod state_service;
#[cfg(feature = "storage-dumper")]
pub mod storage_dumper;
#[cfg(feature = "tokens-tracker")]
pub mod tokens_tracker;

// Re-export common types
pub use neo_extensions::plugin::{Plugin, PluginCategory, PluginContext, PluginEvent, PluginInfo};

/// Plugin collection for easy registration
pub struct PluginCollection;

impl PluginCollection {
    /// Get all available plugins (matches C# Neo plugin collection exactly)
    pub fn all_plugins() -> Vec<Box<dyn Plugin>> {
        #[allow(unused_mut)]
        let mut plugins: Vec<Box<dyn Plugin>> = Vec::new();

        #[cfg(feature = "application-logs")]
        {
            plugins.push(Box::new(application_logs::ApplicationLogsPlugin::new()));
        }
        #[cfg(feature = "dbft")]
        {
            plugins.push(Box::new(dbft_plugin::DBFTPlugin::new()));
        }
        #[cfg(feature = "leveldb-store")]
        {
            plugins.push(Box::new(leveldb_store::LevelDBStore::new(
                leveldb_store::LevelDBStoreSettings::default(),
            )));
        }
        #[cfg(feature = "oracle")]
        {
            plugins.push(Box::new(oracle_service::OracleServicePlugin::new()));
        }
        #[cfg(feature = "rest-server")]
        {
            plugins.push(Box::new(rest_server::RestServerPlugin::new()));
        }
        #[cfg(feature = "rocksdb-store")]
        {
            plugins.push(Box::new(rocksdb_store::RocksDBStore::new()));
        }
        #[cfg(feature = "rpc-server")]
        {
            plugins.push(Box::new(RpcServerPlugin::new()));
        }
        #[cfg(feature = "sign-client")]
        {
            plugins.push(Box::new(sign_client::SignClient::new(
                sign_client::SignClientSettings::default(),
            )));
        }
        #[cfg(feature = "sqlite-wallet")]
        {
            plugins.push(Box::new(sqlite_wallet::SqliteWalletPlugin::new()));
        }
        #[cfg(feature = "state-service")]
        {
            plugins.push(Box::new(state_service::StateServicePlugin::new()));
        }
        #[cfg(feature = "storage-dumper")]
        {
            plugins.push(Box::new(storage_dumper::StorageDumper::new(
                storage_dumper::StorageDumperSettings::default(),
            )));
        }
        #[cfg(feature = "tokens-tracker")]
        {
            plugins.push(Box::new(tokens_tracker::TokensTrackerPlugin::new()));
        }

        plugins
    }

    /// Get core plugins (essential for most deployments)
    pub fn core_plugins() -> Vec<Box<dyn Plugin>> {
        #[allow(unused_mut)]
        let mut plugins: Vec<Box<dyn Plugin>> = Vec::new();

        #[cfg(feature = "dbft")]
        {
            plugins.push(Box::new(dbft_plugin::DBFTPlugin::new()));
        }
        #[cfg(feature = "rpc-server")]
        {
            plugins.push(Box::new(RpcServerPlugin::new()));
        }
        #[cfg(feature = "tokens-tracker")]
        {
            plugins.push(Box::new(tokens_tracker::TokensTrackerPlugin::new()));
        }

        plugins
    }

    /// Get RPC-related plugins
    pub fn rpc_plugins() -> Vec<Box<dyn Plugin>> {
        #[allow(unused_mut)]
        let mut plugins: Vec<Box<dyn Plugin>> = Vec::new();

        #[cfg(feature = "rpc-server")]
        {
            plugins.push(Box::new(RpcServerPlugin::new()));
        }
        #[cfg(feature = "rest-server")]
        {
            plugins.push(Box::new(rest_server::RestServerPlugin::new()));
        }
        #[cfg(feature = "state-service")]
        {
            plugins.push(Box::new(state_service::StateServicePlugin::new()));
        }
        #[cfg(feature = "tokens-tracker")]
        {
            plugins.push(Box::new(tokens_tracker::TokensTrackerPlugin::new()));
        }

        plugins
    }

    /// Get utility plugins
    pub fn utility_plugins() -> Vec<Box<dyn Plugin>> {
        #[allow(unused_mut)]
        let mut plugins: Vec<Box<dyn Plugin>> = Vec::new();

        #[cfg(feature = "sqlite-wallet")]
        {
            plugins.push(Box::new(sqlite_wallet::SqliteWalletPlugin::new()));
        }

        plugins
    }
}
#[cfg(feature = "rpc-server")]
use crate::rpc_server::rpc_server_plugin::RpcServerPlugin;
