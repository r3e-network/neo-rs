use crate::application_logs::ApplicationLogsService;
use crate::plugins::tokens_tracker::TokensTrackerService;
use crate::server::rpc_server::RpcServer;
use crate::server::rpc_server_utilities::response::{plugin_entry_to_json, plugins_to_json};
use neo_indexer::IndexerService;
use serde_json::Value;

impl RpcServer {
    /// List built-in service plugins for Neo RPC compatibility.
    pub fn list_plugins(&self) -> Value {
        let compat = list_plugins_compat();
        let version = plugin_version(compat);
        let persistence_interfaces = ["IPersistencePlugin"];
        let storage_interfaces = ["IStoragePlugin"];
        let plugin_entry =
            |name: &str, interfaces: &[&str]| plugin_entry_to_json(name, &version, interfaces);
        let mut plugins = Vec::new();

        plugins.push(plugin_entry("RpcServer", &[]));

        if self
            .system()
            .get_service::<ApplicationLogsService>()
            .is_some()
        {
            plugins.push(plugin_entry("ApplicationLogs", &persistence_interfaces));
        }
        if self.system().state_store().is_some() {
            plugins.push(plugin_entry("StateService", &persistence_interfaces));
        }
        if self
            .system()
            .get_service::<TokensTrackerService>()
            .is_some()
        {
            let name = match compat {
                ListPluginsCompat::Fixture => "RpcNep17Tracker",
                ListPluginsCompat::Runtime => "TokensTracker",
            };
            plugins.push(plugin_entry(name, &persistence_interfaces));
        }
        if self.system().get_service::<IndexerService>().is_some() {
            plugins.push(plugin_entry("NeoIndexer", &persistence_interfaces));
        }

        // The reth-style Node owns an unnamed store; storage-plugin naming
        // follows the configured compatibility mode.
        let store_name = match compat {
            ListPluginsCompat::Fixture => "LevelDBStore".to_string(),
            ListPluginsCompat::Runtime => "memory".to_string(),
        };
        if !store_name.eq_ignore_ascii_case("memory") {
            plugins.push(plugin_entry(&store_name, &storage_interfaces));
        }

        plugins_to_json(plugins)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListPluginsCompat {
    Runtime,
    Fixture,
}

fn list_plugins_compat() -> ListPluginsCompat {
    let Ok(raw) = std::env::var("NEO_LISTPLUGINS_COMPAT") else {
        return ListPluginsCompat::Runtime;
    };
    match raw.trim().to_ascii_lowercase().as_str() {
        "fixture" | "fixtures" | "legacy" => ListPluginsCompat::Fixture,
        _ => ListPluginsCompat::Runtime,
    }
}

fn plugin_version(compat: ListPluginsCompat) -> String {
    if let Ok(value) = std::env::var("NEO_PLUGIN_VERSION") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return normalize_version(trimmed);
        }
    }
    if compat == ListPluginsCompat::Fixture {
        return "3.0.0.0".to_string();
    }
    normalize_version(env!("CARGO_PKG_VERSION"))
}

fn normalize_version(version: &str) -> String {
    let mut parts = version.split('.').collect::<Vec<_>>();
    if parts.len() == 3
        && parts
            .iter()
            .all(|part| part.chars().all(|ch| ch.is_ascii_digit()))
    {
        parts.push("0");
        parts.join(".")
    } else {
        version.to_string()
    }
}
