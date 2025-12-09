use bs58;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::runtime::{Handle, Runtime};
use tokio::task::block_in_place;

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
    pub fn list_plugins(&self) -> Value {
        let fut = neo_core::extensions::plugin::global_plugin_infos();
        let infos = match Handle::try_current() {
            Ok(handle) => block_in_place(|| handle.block_on(fut)),
            Err(_) => {
                let rt = Runtime::new().expect("failed to build tokio runtime");
                rt.block_on(fut)
            }
        };

        let items: Vec<Value> = infos
            .into_iter()
            .map(|info| {
                json!({
                    "name": info.name,
                    "version": info.version,
                    "interfaces": Vec::<Value>::new(),
                    "category": format!("{:?}", info.category),
                })
            })
            .collect();

        Value::Array(items)
    }

    pub fn validate_address(&self, address: &str) -> Value {
        let address = address.trim();
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
