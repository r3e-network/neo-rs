//! Optional jsonrpsee adapter for Neo JSON-RPC handlers.
//!
//! # Security parity warning
//!
//! This module exposes the Neo handlers through a bare [`jsonrpsee`]
//! [`RpcModule`]. A module only owns per-method dispatch: it cannot see the
//! HTTP request, so it cannot reproduce everything the warp front end does.
//! The adapter intentionally enforces the parts of [`RpcServerConfig`] that are
//! evaluable at method level and refuses to build when a control it cannot
//! implement is required:
//!
//! * `DisabledMethods` — applied at dispatch: the methods stay registered so
//!   callers receive Neo's `AccessDenied` error, exactly as the warp front end
//!   reports them, instead of a generic "method not found". The configured
//!   names are matched case-insensitively and merged with any caller-supplied
//!   deny list.
//! * `RpcUser`/`RpcPass` — the module has no access to the `Authorization`
//!   header, so building fails with
//!   [`JsonRpseeAdapterError::AuthenticationRequired`] unless the caller
//!   explicitly states that authentication is terminated in front of the
//!   jsonrpsee server (see [`build_jsonrpsee_module_with_external_auth`]).
//!   Authenticated methods are never registered here.
//! * `MaxRequestsPerSecond`/`RateLimitBurst` — applied as a single aggregate
//!   ceiling (`per-client rate × MaxConcurrentConnections`) because the module
//!   API exposes no peer identity to key a per-client limiter on.
//! * JSON nesting depth — `params` reach the module as unparsed text and are
//!   measured before they are parsed, against the limit of the warp front end
//!   and counted the way it counts: as part of the request they came in. Over
//!   the limit the call is refused with the warp front end's error and its
//!   handler does not run. A method call cannot tell whether it arrived in a
//!   batch, where the batch array adds a level; [`JsonRpseeBatchLimits`] is
//!   the RPC middleware that accounts for it. Members of a request that
//!   jsonrpsee does not know are not measured: it skips them without building
//!   anything from them, where the warp front end counts them too.
//! * `MaxRequestBodySize`, `MaxBatchSize` — a module never sees the request
//!   body, so the jsonrpsee server has to enforce these: build its settings
//!   with [`jsonrpsee_server_config`]. As a backstop for a server configured
//!   some other way, `params` longer than `MaxRequestBodySize` are refused
//!   at dispatch.
//! * TLS, CORS, connect limits, the admission limit, timeouts (the request
//!   body deadlines included) and compression are **not** inherited; they
//!   must be provided by the `jsonrpsee` server/service layer.
//!
//! A server that carries the limits above is built like this:
//!
//! ```no_run
//! # use jsonrpsee::core::middleware::RpcServiceBuilder;
//! # use jsonrpsee::server::{Server, ServerHandle};
//! # use neo_rpc_server::{
//! #     JsonRpseeBatchLimits, RpcServer, RpcServerConfig, build_jsonrpsee_module,
//! #     jsonrpsee_server_config,
//! # };
//! # use parking_lot::RwLock;
//! # use std::sync::Weak;
//! # async fn serve(
//! #     rpc_server: Weak<RwLock<RpcServer>>,
//! #     config: RpcServerConfig,
//! # ) -> Result<ServerHandle, Box<dyn std::error::Error>> {
//! let server = Server::builder()
//!     .set_config(jsonrpsee_server_config(&config).build())
//!     .set_rpc_middleware(RpcServiceBuilder::new().layer_fn(JsonRpseeBatchLimits::new))
//!     .build("127.0.0.1:10332")
//!     .await?;
//! let handle = server.start(build_jsonrpsee_module(rpc_server, &config)?);
//! # Ok(handle)
//! # }
//! ```
//!
//! Treat the adapter as a convenience for embedding the handlers in an
//! existing, already-hardened jsonrpsee deployment. Do not expose it directly
//! to untrusted networks: use the warp server (`RpcServer::start_rpc_server`)
//! or place this module behind external middleware that supplies the missing
//! controls.

use super::middleware::{GovernorRateLimiter, RateLimitConfig};
use super::routes::{
    invoke_rpc_handler, max_post_body_len, params_exceed_max_depth, resolve_rpc_handler,
};
use super::rpc_error::RpcError;
use super::rpc_server::RpcServer;
use super::rpc_server_settings::RpcServerConfig;
use jsonrpsee::core::RegisterMethodError;
use jsonrpsee::core::middleware::{Batch, Notification, Request, ResponseFuture, RpcServiceT};
use jsonrpsee::server::{BatchRequestConfig, ServerConfig, ServerConfigBuilder};
use jsonrpsee::types::{ErrorObjectOwned, Id, Params};
use jsonrpsee::{MethodResponse, RpcModule};
use parking_lot::RwLock;
use serde_json::Value;
use std::collections::HashSet;
use std::fmt;
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Weak};

/// Synthetic limiter key: every adapter request shares one aggregate bucket
/// because the module API carries no peer identity.
const AGGREGATE_CLIENT: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);

/// Failure raised while building the optional jsonrpsee adapter.
#[derive(Debug)]
pub enum JsonRpseeAdapterError {
    /// jsonrpsee rejected a method registration.
    RegisterMethod(RegisterMethodError),
    /// Credentials are configured but the adapter cannot verify them; the
    /// caller must terminate authentication in front of the module.
    AuthenticationRequired,
}

impl fmt::Display for JsonRpseeAdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RegisterMethod(err) => write!(formatter, "failed to register RPC method: {err}"),
            Self::AuthenticationRequired => formatter.write_str(
                "jsonrpsee adapter cannot enforce RpcUser/RpcPass authentication; \
                 enable external authentication and build with \
                 build_jsonrpsee_module_with_external_auth",
            ),
        }
    }
}

impl std::error::Error for JsonRpseeAdapterError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::RegisterMethod(err) => Some(err),
            Self::AuthenticationRequired => None,
        }
    }
}

impl From<RegisterMethodError> for JsonRpseeAdapterError {
    fn from(err: RegisterMethodError) -> Self {
        Self::RegisterMethod(err)
    }
}

/// Shared context used by the optional jsonrpsee RPC module.
#[derive(Clone)]
pub struct JsonRpseeContext {
    server: Weak<RwLock<RpcServer>>,
    disabled: Arc<HashSet<String>>,
    rate_limiter: Option<Arc<GovernorRateLimiter>>,
    /// `MaxRequestBodySize`: no `params` can be longer than the body they
    /// came in.
    max_request_len: u64,
}

impl JsonRpseeContext {
    #[must_use]
    pub fn new(server: Weak<RwLock<RpcServer>>, disabled: Arc<HashSet<String>>) -> Self {
        Self {
            server,
            disabled,
            rate_limiter: None,
            max_request_len: max_post_body_len(&RpcServerConfig::default()),
        }
    }

    /// Methods this adapter must not serve.
    #[must_use]
    pub fn disabled_methods(&self) -> &HashSet<String> {
        self.disabled.as_ref()
    }

    fn check_rate_limit(&self, method: &str) -> Result<(), ErrorObjectOwned> {
        let Some(limiter) = self.rate_limiter.as_ref() else {
            return Ok(());
        };
        if limiter
            .check_for_method(AGGREGATE_CLIENT, method)
            .is_blocked()
        {
            return Err(error_object(RpcError::too_many_requests()));
        }
        Ok(())
    }
}

/// Builds the adapter, inheriting `DisabledMethods`, the credential
/// requirement and the rate limit ceiling from `config`.
pub fn build_jsonrpsee_module(
    server: Weak<RwLock<RpcServer>>,
    config: &RpcServerConfig,
) -> Result<RpcModule<JsonRpseeContext>, JsonRpseeAdapterError> {
    build_jsonrpsee_module_with_disabled(server, config, Arc::new(HashSet::new()))
}

/// Builds the adapter with extra disabled methods merged into the configured
/// set.
pub fn build_jsonrpsee_module_with_disabled(
    server: Weak<RwLock<RpcServer>>,
    config: &RpcServerConfig,
    extra_disabled: Arc<HashSet<String>>,
) -> Result<RpcModule<JsonRpseeContext>, JsonRpseeAdapterError> {
    build_module(server, config, extra_disabled, false)
}

/// Builds the adapter for a deployment where authentication is enforced by
/// middleware in front of the jsonrpsee server.
///
/// Only use this when the surrounding server verifies `RpcUser`/`RpcPass`
/// before requests reach the module: the module itself cannot see the
/// `Authorization` header.
pub fn build_jsonrpsee_module_with_external_auth(
    server: Weak<RwLock<RpcServer>>,
    config: &RpcServerConfig,
    extra_disabled: Arc<HashSet<String>>,
) -> Result<RpcModule<JsonRpseeContext>, JsonRpseeAdapterError> {
    build_module(server, config, extra_disabled, true)
}

/// Settings for the jsonrpsee server that serves the module, carrying the
/// request limits of `config` that only the server can enforce:
/// `MaxRequestBodySize` and `MaxBatchSize`, as the warp front end applies
/// them.
///
/// Returned as a builder so the caller can add what else the deployment
/// needs before building it.
#[must_use]
pub fn jsonrpsee_server_config(config: &RpcServerConfig) -> ServerConfigBuilder {
    let max_body = u32::try_from(max_post_body_len(config)).unwrap_or(u32::MAX);
    let max_batch = u32::try_from(config.max_batch_size).unwrap_or(u32::MAX);
    ServerConfig::builder()
        .max_request_body_size(max_body)
        .set_batch_request_config(BatchRequestConfig::Limit(max_batch))
}

/// RPC middleware that holds a batch to the depth limit of the warp front
/// end.
///
/// The module checks every call on its own, but a call does not know it came
/// in a batch, where the batch array puts its `params` one level deeper. This
/// counts that level and, like the warp front end, refuses the whole batch
/// when any of it is too deep: none of its calls run.
///
/// Install it with
/// `RpcServiceBuilder::new().layer_fn(JsonRpseeBatchLimits::new)`.
#[derive(Clone, Debug)]
pub struct JsonRpseeBatchLimits<S> {
    service: S,
}

impl<S> JsonRpseeBatchLimits<S> {
    pub const fn new(service: S) -> Self {
        Self { service }
    }
}

impl<S> RpcServiceT for JsonRpseeBatchLimits<S>
where
    S: RpcServiceT<
            MethodResponse = MethodResponse,
            NotificationResponse = MethodResponse,
            BatchResponse = MethodResponse,
        >,
{
    type MethodResponse = MethodResponse;
    type NotificationResponse = MethodResponse;
    type BatchResponse = MethodResponse;

    fn call<'a>(
        &self,
        request: Request<'a>,
    ) -> impl Future<Output = Self::MethodResponse> + Send + 'a {
        self.service.call(request)
    }

    fn batch<'a>(&self, batch: Batch<'a>) -> impl Future<Output = Self::BatchResponse> + Send + 'a {
        let too_deep = batch.iter().flatten().any(|entry| {
            entry
                .params()
                .is_some_and(|params| params_exceed_max_depth(params.get(), true))
        });
        if too_deep {
            return ResponseFuture::ready(MethodResponse::error(
                Id::Null,
                error_object(RpcError::bad_request()),
            ));
        }
        ResponseFuture::future(self.service.batch(batch))
    }

    fn notification<'a>(
        &self,
        notification: Notification<'a>,
    ) -> impl Future<Output = Self::NotificationResponse> + Send + 'a {
        self.service.notification(notification)
    }
}

fn build_module(
    server: Weak<RwLock<RpcServer>>,
    config: &RpcServerConfig,
    extra_disabled: Arc<HashSet<String>>,
    external_auth: bool,
) -> Result<RpcModule<JsonRpseeContext>, JsonRpseeAdapterError> {
    if !config.rpc_user.trim().is_empty() && !external_auth {
        return Err(JsonRpseeAdapterError::AuthenticationRequired);
    }

    let disabled = merge_disabled(config, extra_disabled);
    let rate_limiter = build_rate_limiter(config);
    let methods = registered_public_methods(&server);

    let mut context = JsonRpseeContext::new(server, Arc::clone(&disabled));
    context.rate_limiter = rate_limiter;
    context.max_request_len = max_post_body_len(config);

    let mut module = RpcModule::new(context);
    for method in methods {
        register_neo_method(&mut module, method)?;
    }
    Ok(module)
}

fn merge_disabled(
    config: &RpcServerConfig,
    extra_disabled: Arc<HashSet<String>>,
) -> Arc<HashSet<String>> {
    let mut disabled: HashSet<String> = config
        .disabled_methods
        .iter()
        .map(|name| name.to_ascii_lowercase())
        .collect();
    disabled.extend(extra_disabled.iter().map(|name| name.to_ascii_lowercase()));
    Arc::new(disabled)
}

/// Aggregate request ceiling for the module.
///
/// The module API exposes no peer identity, so the configured per-client rate
/// is scaled by the allowed number of concurrent connections: the result bounds
/// total throughput without throttling a single well-behaved client.
fn build_rate_limiter(config: &RpcServerConfig) -> Option<Arc<GovernorRateLimiter>> {
    if config.max_requests_per_second == 0 {
        return None;
    }
    let connections = u32::try_from(config.max_concurrent_connections)
        .unwrap_or(u32::MAX)
        .max(1);
    let burst = if config.rate_limit_burst == 0 {
        config.max_requests_per_second
    } else {
        config.rate_limit_burst
    };
    let limiter = GovernorRateLimiter::new(RateLimitConfig {
        max_rps: config.max_requests_per_second.saturating_mul(connections),
        burst: burst.saturating_mul(connections),
    });
    limiter.is_enabled().then(|| Arc::new(limiter))
}

fn registered_public_methods(server: &Weak<RwLock<RpcServer>>) -> Vec<String> {
    let Some(server) = server.upgrade() else {
        return Vec::new();
    };

    let server = server.read();
    let handlers = server.handlers_guard();
    let mut methods = handlers
        .values()
        .filter(|handler| !handler.descriptor().requires_auth())
        .map(|handler| handler.descriptor().name.clone())
        .collect::<Vec<_>>();
    methods.sort_unstable();
    methods.dedup();
    methods
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
                let params = parse_array_params(params, context.max_request_len)?;
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
    context.check_rate_limit(method)?;
    let (server, handler) = resolve_rpc_handler(&context.server, context.disabled.as_ref(), method)
        .map_err(error_object)?;

    invoke_rpc_handler(&server, handler, method, params).map_err(error_object)
}

fn parse_array_params(params: Params<'_>, max_len: u64) -> Result<Vec<Value>, ErrorObjectOwned> {
    let Some(raw) = params.as_str() else {
        return Ok(Vec::new());
    };

    if raw.is_empty() {
        return Ok(Vec::new());
    }

    // jsonrpsee hands `params` over unparsed, so both limits are checked on
    // the text, before anything is built from it.
    if raw.len() as u64 > max_len || params_exceed_max_depth(raw, false) {
        return Err(error_object(RpcError::bad_request()));
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
