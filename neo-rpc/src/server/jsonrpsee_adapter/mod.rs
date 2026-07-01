//! # neo-rpc::server::jsonrpsee_adapter
//!
//! jsonrpsee integration that exposes the internal RPC registry.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `jsonrpsee_adapter`: jsonrpsee module construction and context glue.

use super::dispatch::Dispatch;
use super::rpc_error::RpcError;
use super::rpc_server::{RPC_ERR_TOTAL, RPC_REQ_TOTAL, RpcServer};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use jsonrpsee::RpcModule;
use jsonrpsee::core::RegisterMethodError;
use jsonrpsee::server::Extensions;
use jsonrpsee::types::{ErrorObjectOwned, Params};
use parking_lot::RwLock;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::{Arc, Weak};
use subtle::ConstantTimeEq;

/// Authentication marker inserted by the HTTP middleware after Basic auth succeeds.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RpcAuthState {
    authenticated: bool,
}

impl RpcAuthState {
    /// Return an authenticated request marker.
    #[must_use]
    pub(crate) const fn authenticated() -> Self {
        Self {
            authenticated: true,
        }
    }

    /// Return whether the transport authenticated this request.
    #[must_use]
    pub(crate) const fn is_authenticated(self) -> bool {
        self.authenticated
    }
}

/// Shared context used by the jsonrpsee RPC module.
#[derive(Clone)]
pub struct JsonRpseeContext {
    server: Weak<RwLock<RpcServer>>,
    disabled: Arc<HashSet<String>>,
}

impl JsonRpseeContext {
    /// Create a jsonrpsee module context with the live RPC server reference.
    #[must_use]
    pub fn new(server: Weak<RwLock<RpcServer>>, disabled: Arc<HashSet<String>>) -> Self {
        Self { server, disabled }
    }
}

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
/// read the `Weak<RwLock<RpcServer>>` to collect method names — that re-locks
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
                let result = parse_array_params(params)
                    .and_then(|params| dispatch(&context, method, params.as_slice(), &extensions));
                if result.is_err() {
                    RPC_ERR_TOTAL.inc();
                }
                result
            },
        )
        .map(|_| ())
}

fn dispatch(
    context: &JsonRpseeContext,
    method: &str,
    params: &[Value],
    extensions: &Extensions,
) -> Result<Value, ErrorObjectOwned> {
    let (server, handler) =
        Dispatch::resolve_rpc_handler(&context.server, context.disabled.as_ref(), method)
            .map_err(error_object)?;

    if handler.descriptor().requires_auth() {
        let authenticated = extensions
            .get::<RpcAuthState>()
            .is_some_and(|state| state.is_authenticated());
        if !server.read().rpc_auth_configured() || !authenticated {
            return Err(error_object(RpcError::access_denied()));
        }
    }

    Dispatch::invoke_rpc_handler(&server, handler, method, params).map_err(error_object)
}

/// Verify an HTTP Basic Authorization header against the configured credentials.
#[must_use]
pub(crate) fn verify_basic_auth_header(header: Option<&str>, user: &str, password: &str) -> bool {
    let Some(header) = header else {
        return false;
    };
    let Some((scheme, encoded)) = header.trim().split_once(' ') else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("basic") {
        return false;
    }
    let Ok(decoded) = BASE64_STANDARD.decode(encoded.trim()) else {
        return false;
    };
    let expected = format!("{user}:{password}");
    decoded.as_slice().ct_eq(expected.as_bytes()).into()
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
