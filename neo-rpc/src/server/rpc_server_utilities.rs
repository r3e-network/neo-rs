use bs58;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;

use neo_core::UInt160;

use super::rpc_error::RpcError;
use super::rpc_exception::RpcException;
use super::rpc_method_attribute::RpcMethodDescriptor;
use super::rpc_server::{RpcHandler, RpcServer};
use neo_core::application_logs::ApplicationLogsService;
use neo_core::tokens_tracker::TokensTrackerService;

pub struct RpcServerUtilities;

impl RpcServerUtilities {
    pub fn register_handlers() -> Vec<RpcHandler> {
        vec![
            Self::handler("listplugins", Self::list_plugins_handler),
            Self::handler("validateaddress", Self::validate_address_handler),
        ]
    }

    fn handler(
        name: &'static str,
        func: fn(&RpcServer, &[Value]) -> Result<Value, RpcException>,
    ) -> RpcHandler {
        RpcHandler::new(RpcMethodDescriptor::new(name), Arc::new(func))
    }

    fn list_plugins_handler(server: &RpcServer, _params: &[Value]) -> Result<Value, RpcException> {
        Ok(server.list_plugins())
    }

    fn validate_address_handler(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let address = params.first().and_then(|v| v.as_str()).ok_or_else(|| {
            RpcException::from(RpcError::invalid_params().with_data("address parameter required"))
        })?;
        Ok(server.validate_address(address))
    }
}

impl RpcServer {
    /// List plugins - returns built-in services for API compatibility.
    pub fn list_plugins(&self) -> Value {
        let compat = list_plugins_compat();
        let version = plugin_version(compat);
        let persistence_interfaces = ["IPersistencePlugin"];
        let storage_interfaces = ["IStoragePlugin"];
        let plugin_entry = |name: &str, interfaces: &[&str]| {
            let interface_values = interfaces
                .iter()
                .map(|value| Value::String((*value).to_string()))
                .collect::<Vec<_>>();
            json!({
                "name": name,
                "version": version,
                "interfaces": interface_values,
            })
        };
        let mut plugins = Vec::new();

        plugins.push(plugin_entry("RpcServer", &[]));

        if self
            .system()
            .get_service::<ApplicationLogsService>()
            .ok()
            .flatten()
            .is_some()
        {
            plugins.push(plugin_entry("ApplicationLogs", &persistence_interfaces));
        }

        if self.system().state_store().ok().flatten().is_some() {
            plugins.push(plugin_entry("StateService", &persistence_interfaces));
        }

        if self
            .system()
            .get_service::<TokensTrackerService>()
            .ok()
            .flatten()
            .is_some()
        {
            let name = match compat {
                ListPluginsCompat::Fixture => "RpcNep17Tracker",
                ListPluginsCompat::Runtime => "TokensTracker",
            };
            plugins.push(plugin_entry(name, &persistence_interfaces));
        }

        let store_provider = self.system().store_provider();
        let store_name = match compat {
            ListPluginsCompat::Fixture => "LevelDBStore".to_string(),
            ListPluginsCompat::Runtime => store_provider.name().to_string(),
        };
        if !store_name.eq_ignore_ascii_case("memory") {
            plugins.push(plugin_entry(&store_name, &storage_interfaces));
        }

        plugins.sort_by(|a, b| {
            let a_name = a.get("name").and_then(Value::as_str).unwrap_or("");
            let b_name = b.get("name").and_then(Value::as_str).unwrap_or("");
            a_name.cmp(b_name)
        });

        Value::Array(plugins)
    }

    pub fn validate_address(&self, address: &str) -> Value {
        let address_version = self.system().settings().address_version;
        let is_valid = parse_address_with_version(address, address_version).is_ok();

        json!({
            "address": address,
            "isvalid": is_valid,
        })
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
        return parts.join(".");
    }
    version.to_string()
}

fn parse_address_with_version(address: &str, version: u8) -> Result<UInt160, ()> {
    let decoded = bs58::decode(address).into_vec().map_err(|_| ())?;
    if decoded.len() != 25 {
        return Err(());
    }

    if decoded[0] != version {
        return Err(());
    }

    let data = &decoded[..21];
    let checksum = &decoded[21..];
    let first_hash = Sha256::digest(data);
    let second_hash = Sha256::digest(first_hash);
    if checksum != &second_hash[..4] {
        return Err(());
    }

    UInt160::from_bytes(&decoded[1..21]).map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::rcp_server_settings::RpcServerConfig;
    use crate::server::rpc_server::RpcHandler;
    use neo_core::neo_system::NeoSystem;
    use neo_core::protocol_settings::ProtocolSettings;

    fn find_handler<'a>(handlers: &'a [RpcHandler], name: &str) -> &'a RpcHandler {
        handlers
            .iter()
            .find(|handler| handler.descriptor().name.eq_ignore_ascii_case(name))
            .unwrap_or_else(|| panic!("handler {} not found", name))
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn list_plugins_returns_empty() {
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system, RpcServerConfig::default());
        let handlers = RpcServerUtilities::register_handlers();
        let handler = find_handler(&handlers, "listplugins");

        let result = (handler.callback())(&server, &[]).expect("listplugins");
        let plugins = result.as_array().expect("listplugins array");
        assert_eq!(plugins.len(), 1);
        let entry = plugins[0].as_object().expect("plugin entry");
        assert_eq!(entry.get("name").and_then(Value::as_str), Some("RpcServer"));
        assert!(entry.get("interfaces").is_some());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn validate_address_variants() {
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system, RpcServerConfig::default());
        let handlers = RpcServerUtilities::register_handlers();
        let handler = find_handler(&handlers, "validateaddress");

        let valid_address = UInt160::zero().to_address();
        let params = [Value::String(valid_address.clone())];
        let result = (handler.callback())(&server, &params).expect("validateaddress");
        let obj = result.as_object().expect("validateaddress object");
        assert_eq!(
            obj.get("address").and_then(Value::as_str),
            Some(valid_address.as_str())
        );
        assert_eq!(obj.get("isvalid").and_then(Value::as_bool), Some(true));

        let mut invalid_checksum = valid_address.clone();
        let last = invalid_checksum.pop().expect("address has last char");
        invalid_checksum.push(if last == 'A' { 'B' } else { 'A' });

        for invalid in [
            String::new(),
            UInt160::zero().to_string(),
            invalid_checksum,
            valid_address[..valid_address.len().saturating_sub(1)].to_string(),
            format!("{}X", valid_address),
        ] {
            let params = [Value::String(invalid.clone())];
            let result = (handler.callback())(&server, &params).expect("validateaddress");
            let obj = result.as_object().expect("validateaddress object");
            assert_eq!(
                obj.get("address").and_then(Value::as_str),
                Some(invalid.as_str())
            );
            assert_eq!(obj.get("isvalid").and_then(Value::as_bool), Some(false));
        }

        let spaced = format!(" {} ", valid_address);
        let params = [Value::String(spaced.clone())];
        let result = (handler.callback())(&server, &params).expect("validateaddress");
        let obj = result.as_object().expect("validateaddress object");
        assert_eq!(
            obj.get("address").and_then(Value::as_str),
            Some(spaced.as_str())
        );
        assert_eq!(obj.get("isvalid").and_then(Value::as_bool), Some(false));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn validate_address_requires_param() {
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let server = RpcServer::new(system, RpcServerConfig::default());
        let handlers = RpcServerUtilities::register_handlers();
        let handler = find_handler(&handlers, "validateaddress");

        let err = (handler.callback())(&server, &[]).expect_err("missing param");
        let rpc_error: RpcError = err.into();
        assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
    }
}
