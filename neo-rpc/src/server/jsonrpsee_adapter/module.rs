//! jsonrpsee module registration for transport-visible RPC methods.

use super::codec::parse_array_params;
use super::context::JsonRpseeContext;
use super::dispatch::dispatch_jsonrpsee_request;
use crate::server::rpc_server::{RPC_ERR_TOTAL, RPC_REQ_TOTAL, RpcServer};
use jsonrpsee::RpcModule;
use jsonrpsee::core::RegisterMethodError;
use jsonrpsee::types::ErrorObjectOwned;
use parking_lot::RwLock;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::{Arc, Weak};

/// Build a jsonrpsee module from the transport-visible methods registered on `server`.
pub fn build_jsonrpsee_module(
    server: Weak<RwLock<RpcServer>>,
) -> Result<RpcModule<JsonRpseeContext>, RegisterMethodError> {
    build_jsonrpsee_module_with_disabled(server, Arc::new(HashSet::new()))
}

/// Build a jsonrpsee module while excluding disabled method names.
pub fn build_jsonrpsee_module_with_disabled(
    server: Weak<RwLock<RpcServer>>,
    disabled: Arc<HashSet<String>>,
) -> Result<RpcModule<JsonRpseeContext>, RegisterMethodError> {
    let methods = registered_public_methods(&server);
    build_jsonrpsee_module_with_methods(server, disabled, methods)
}

/// Builds the jsonrpsee module from a **precomputed** transport-method list.
///
/// `RpcServer::start_rpc_server` runs under the server's own write lock (it is
/// invoked as `server.write().start_rpc_server(...)`), so it must NOT upgrade +
/// read the `Weak<RwLock<RpcServer>>` to collect method names; that re-locks
/// the same non-reentrant `parking_lot::RwLock` on the same thread and
/// deadlocks RPC startup. It collects the names from `&self` via
/// [`RpcServer::transport_method_names`] and passes them in here instead.
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
    server.read().transport_method_names()
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
            move |params, context, extensions| {
                RPC_REQ_TOTAL.inc();
                let result = parse_array_params(params).and_then(|params| {
                    dispatch_jsonrpsee_request(&context, method, params.as_slice(), &extensions)
                });
                if result.is_err() {
                    RPC_ERR_TOTAL.inc();
                }
                result
            },
        )
        .map(|_| ())
}
