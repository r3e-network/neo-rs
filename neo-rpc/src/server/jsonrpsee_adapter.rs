//! Canonical JSON-RPC transport for the Neo RPC server (jsonrpsee-based).
//!
//! Replaces the previous `warp`-based glue in `routes/`. The dispatch
//! logic (`resolve_rpc_handler` / `invoke_rpc_handler`) now lives in
//! `super::dispatch`, so the same panic-safe handler invocation is
//! reused by both this jsonrpsee module and any future transport.

use super::dispatch::Dispatch;
use super::rpc_error::RpcError;
use super::rpc_server::RpcServer;
use jsonrpsee::RpcModule;
use jsonrpsee::core::RegisterMethodError;
use jsonrpsee::types::{ErrorObjectOwned, Params};
use parking_lot::RwLock;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::{Arc, Weak};

/// Shared context used by the jsonrpsee RPC module.
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
    let methods = registered_public_methods(&server);
    build_jsonrpsee_module_with_methods(server, disabled, methods)
}

/// Builds the jsonrpsee module from a **precomputed** public-method list.
///
/// `RpcServer::start_rpc_server` runs under the server's own write lock (it is
/// invoked as `server.write().start_rpc_server(...)`), so it must NOT upgrade +
/// read the `Weak<RwLock<RpcServer>>` to collect method names — that re-locks
/// the same non-reentrant `parking_lot::RwLock` on the same thread and
/// deadlocks RPC startup. It collects the names from `&self` via
/// [`RpcServer::public_method_names`] and passes them in here instead.
pub fn build_jsonrpsee_module_with_methods(
    server: Weak<RwLock<RpcServer>>,
    disabled: Arc<HashSet<String>>,
    methods: Vec<String>,
) -> Result<RpcModule<JsonRpseeContext>, RegisterMethodError> {
    let mut module = RpcModule::new(JsonRpseeContext::new(server, disabled));
    for method in methods {
        register_neo_method(&mut module, method)?;
    }
    Ok(module)
}

fn registered_public_methods(server: &Weak<RwLock<RpcServer>>) -> Vec<String> {
    let Some(server) = server.upgrade() else {
        return Vec::new();
    };
    server.read().public_method_names()
}

fn register_neo_method(
    module: &mut RpcModule<JsonRpseeContext>,
    method: String,
) -> Result<(), RegisterMethodError> {
    // jsonrpsee stores method names as &'static str; this keeps dynamic RpcHandler names
    // compatible without restoring a separate static method whitelist.
    let method: &'static str = Box::leak(method.into_boxed_str());
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
    let (server, handler) =
        Dispatch::resolve_rpc_handler(&context.server, context.disabled.as_ref(), method)
            .map_err(error_object)?;

    Dispatch::invoke_rpc_handler(&server, handler, method, params).map_err(error_object)
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
