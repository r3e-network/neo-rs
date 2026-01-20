use bs58;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;

use neo_core::UInt160;

use super::rpc_error::RpcError;
use super::rpc_exception::RpcException;
use super::rpc_method_attribute::RpcMethodDescriptor;
use super::rpc_server::{RpcHandler, RpcServer};

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
    /// List plugins - returns empty in the new architecture since plugins are integrated
    pub fn list_plugins(&self) -> Value {
        // In the new architecture, plugins are integrated directly into the node
        // This endpoint is kept for API compatibility but returns an empty list
        Value::Array(Vec::new())
    }

    pub fn validate_address(&self, address: &str) -> Value {
        let address_version = self.system().settings().address_version;
        let is_valid = parse_address_with_version(address, address_version)
            .or_else(|_| parse_address_with_version(address, 0x35))
            .is_ok();

        json!({
            "address": address,
            "isvalid": is_valid,
        })
    }
}

fn parse_address_with_version(address: &str, version: u8) -> Result<UInt160, ()> {
    let mut result = None;
    if UInt160::try_parse(address, &mut result) {
        if let Some(value) = result {
            return Ok(value);
        }
    }

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
        assert!(plugins.is_empty());
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
