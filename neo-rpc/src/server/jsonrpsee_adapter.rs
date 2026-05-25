//! Optional jsonrpsee adapter for Neo JSON-RPC handlers.

use super::routes::{invoke_rpc_handler, lookup_rpc_handler};
use super::rpc_error::RpcError;
use super::rpc_server::RpcServer;
use jsonrpsee::core::RegisterMethodError;
use jsonrpsee::types::{ErrorObjectOwned, Params};
use jsonrpsee::RpcModule;
use parking_lot::RwLock;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::{Arc, Weak};

pub const JSONRPSEE_READ_ONLY_METHODS: &[&str] = &[
    "getbestblockhash",
    "getblockcount",
    "getblockheadercount",
    "getnativecontracts",
    "getnextblockvalidators",
    "getcandidates",
    "getconnectioncount",
    "getrawmempool",
    "getversion",
    "listplugins",
];

/// Shared context used by the optional jsonrpsee RPC module.
#[derive(Clone)]
pub struct JsonRpseeContext {
    server: Weak<RwLock<RpcServer>>,
    disabled: Arc<HashSet<String>>,
}

impl JsonRpseeContext {
    #[must_use]
    pub fn new(server: Weak<RwLock<RpcServer>>, disabled: Arc<HashSet<String>>) -> Self {
        Self { server, disabled }
    }
}

pub fn build_jsonrpsee_module(
    server: Weak<RwLock<RpcServer>>,
) -> Result<RpcModule<JsonRpseeContext>, RegisterMethodError> {
    build_jsonrpsee_module_with_disabled(server, Arc::new(HashSet::new()))
}

pub fn build_jsonrpsee_module_with_disabled(
    server: Weak<RwLock<RpcServer>>,
    disabled: Arc<HashSet<String>>,
) -> Result<RpcModule<JsonRpseeContext>, RegisterMethodError> {
    let mut module = RpcModule::new(JsonRpseeContext::new(server, disabled));
    for method in JSONRPSEE_READ_ONLY_METHODS {
        register_neo_method(&mut module, method)?;
    }
    Ok(module)
}

fn register_neo_method(
    module: &mut RpcModule<JsonRpseeContext>,
    method: &'static str,
) -> Result<(), RegisterMethodError> {
    module
        .register_blocking_method::<Result<Value, ErrorObjectOwned>, _>(
            method,
            move |params, context, _| {
                let params = parse_array_params(params)?;
                dispatch(&context, method, params.as_slice())
            },
        )
        .map(|_| ())
}

fn dispatch(
    context: &JsonRpseeContext,
    method: &str,
    params: &[Value],
) -> Result<Value, ErrorObjectOwned> {
    let method_key = method.to_ascii_lowercase();
    if context.disabled.contains(&method_key) {
        return Err(error_object(RpcError::access_denied()));
    }

    let server = context
        .server
        .upgrade()
        .ok_or_else(|| error_object(RpcError::internal_server_error()))?;
    let handler = lookup_rpc_handler(&server, &method_key)
        .ok_or_else(|| error_object(RpcError::method_not_found().with_data(method)))?;

    invoke_rpc_handler(&server, handler, method, params).map_err(error_object)
}

fn parse_array_params(params: Params<'_>) -> Result<Vec<Value>, ErrorObjectOwned> {
    let Some(raw) = params.as_str() else {
        return Ok(Vec::new());
    };

    if raw.is_empty() {
        return Ok(Vec::new());
    }

    match serde_json::from_str::<Value>(raw) {
        Ok(Value::Array(values)) => Ok(values),
        Ok(_) => Err(error_object(RpcError::invalid_request())),
        Err(err) => Err(error_object(
            RpcError::invalid_params().with_data(err.to_string()),
        )),
    }
}

fn error_object(error: RpcError) -> ErrorObjectOwned {
    ErrorObjectOwned::owned(
        error.code(),
        error.error_message(),
        error.data().map(std::string::ToString::to_string),
    )
}
