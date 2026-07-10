use std::sync::Arc;

use serde_json::Value;

use super::RpcServerUtilities;
use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_server::{RpcHandler, RpcServer};
use crate::server::rpc_server_settings::RpcServerConfig;
use neo_config::ProtocolSettings;
use neo_indexer::IndexerService;
use neo_primitives::UInt160;

fn find_handler<'a>(handlers: &'a [RpcHandler], name: &str) -> &'a RpcHandler {
    handlers
        .iter()
        .find(|handler| handler.descriptor().name.eq_ignore_ascii_case(name))
        .unwrap_or_else(|| panic!("handler {} not found", name))
}

fn service_by_name<'a>(services: &'a [Value], name: &str) -> &'a Value {
    services
        .iter()
        .find(|service| {
            service
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|actual| actual == name)
        })
        .unwrap_or_else(|| panic!("service {} not found", name))
}

fn assert_invalid_params(error: RpcException, expected_message: &str) {
    assert_eq!(error.code(), RpcError::invalid_params().code());
    assert!(
        error.to_string().contains(expected_message),
        "unexpected error: {error}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn list_plugins_returns_empty() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerUtilities::register_handlers();
    let handler = find_handler(&handlers, "listplugins");

    let result = (handler.callback())(&server, &[]).expect("listplugins");
    let plugins = result.as_array().expect("listplugins array");
    assert_eq!(plugins.len(), 1);
    let entry = plugins[0].as_object().expect("plugin entry");
    assert_eq!(entry.get("name").and_then(Value::as_str), Some("RpcServer"));
    assert!(entry.get("interfaces").is_some());
    assert!(entry.get("methods").is_none());
    assert!(entry.get("ready").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn list_plugins_includes_indexer_when_registered() {
    let system = crate::server::test_support::test_system_with_services(
        ProtocolSettings::default(),
        crate::server::RpcServices::new().with_indexer(Arc::new(IndexerService::new())),
    );
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerUtilities::register_handlers();
    let handler = find_handler(&handlers, "listplugins");

    let result = (handler.callback())(&server, &[]).expect("listplugins");
    let plugins = result.as_array().expect("listplugins array");
    assert!(plugins.iter().any(|entry| {
        entry
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| name == "NeoIndexer")
    }));
}

#[tokio::test(flavor = "multi_thread")]
async fn list_services_reports_registered_method_groups() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerUtilities::register_handlers();
    let handler = find_handler(&handlers, "listservices");

    let result = (handler.callback())(&server, &[]).expect("listservices");
    let services = result.as_array().expect("listservices array");

    let rpc = service_by_name(services, "RpcServer");
    assert_eq!(rpc["enabled"], true);
    assert_eq!(rpc["ready"], true);
    assert!(rpc["methods"].as_array().is_some_and(|methods| {
        methods.iter().any(|method| method == "listservices")
            && methods.iter().any(|method| method == "listplugins")
    }));

    let indexer = service_by_name(services, "NeoIndexer");
    assert_eq!(indexer["enabled"], false);
    assert_eq!(indexer["ready"], false);
    assert!(indexer["status"].is_null());
    assert!(indexer["methods"].as_array().is_some_and(|methods| {
        methods.iter().any(|method| method == "getindexerstatus")
            && methods
                .iter()
                .any(|method| method == "getcontractnotifications")
    }));
}

#[tokio::test(flavor = "multi_thread")]
async fn list_services_reports_indexer_status_when_registered() {
    let system = crate::server::test_support::test_system_with_services(
        ProtocolSettings::default(),
        crate::server::RpcServices::new().with_indexer(Arc::new(IndexerService::new())),
    );
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerUtilities::register_handlers();
    let handler = find_handler(&handlers, "listservices");

    let result = (handler.callback())(&server, &[]).expect("listservices");
    let services = result.as_array().expect("listservices array");
    let indexer = service_by_name(services, "NeoIndexer");

    assert_eq!(indexer["enabled"], true);
    assert_eq!(indexer["ready"], true);
    assert_eq!(indexer["status"]["persistencemode"], "memory");
    assert_eq!(indexer["status"]["persistent"], false);
    assert_eq!(indexer["status"]["indexedblocks"], 0);
    assert_eq!(indexer["status"]["indexedheight"], Value::Null);
    assert_eq!(indexer["status"]["ledgerheight"], 0);
    assert_eq!(indexer["status"]["blocksbehind"], 1);
    assert_eq!(indexer["status"]["synced"], false);
    assert_eq!(indexer["status"]["applicationlogs"]["enabled"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn utility_inventory_methods_reject_unexpected_parameters() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerUtilities::register_handlers();

    for method in ["listplugins", "listservices"] {
        let handler = find_handler(&handlers, method);
        let error = (handler.callback())(&server, &[Value::from(1_u64)])
            .expect_err("utility inventory method should reject parameters");
        assert_invalid_params(error, &format!("{method} expects no parameters"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn validate_address_variants() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
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
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerUtilities::register_handlers();
    let handler = find_handler(&handlers, "validateaddress");

    let err = (handler.callback())(&server, &[]).expect_err("missing param");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}
