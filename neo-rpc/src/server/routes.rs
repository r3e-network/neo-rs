use super::middleware::{GovernorRateLimiter, RateLimitConfig, RateLimitTier};
use super::rcp_server_settings::{RpcServerConfig, RpcServerSettings, UnhandledExceptionPolicy};
use super::rpc_error::RpcError;
use super::rpc_server::{RpcServer, RPC_ERR_TOTAL, RPC_REQ_TOTAL};

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use bytes::Bytes;
use parking_lot::RwLock;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Weak};
use subtle::ConstantTimeEq;
use tracing::error;
use warp::http::header::{
    HeaderValue, ACCESS_CONTROL_ALLOW_CREDENTIALS, ACCESS_CONTROL_ALLOW_HEADERS,
    ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN, CONTENT_TYPE, VARY,
    WWW_AUTHENTICATE,
};
use warp::http::StatusCode;
use warp::reply::Response as HttpResponse;
use warp::Filter;

const MAX_PARAMS_DEPTH: usize = 32;

#[derive(Clone)]
pub struct BasicAuth {
    user: Vec<u8>,
    pass: Vec<u8>,
}

impl BasicAuth {
    pub fn from_settings(settings: &RpcServerConfig) -> Option<Self> {
        if settings.rpc_user.trim().is_empty() {
            return None;
        }

        Some(Self {
            user: settings.rpc_user.as_bytes().to_vec(),
            pass: settings.rpc_pass.as_bytes().to_vec(),
        })
    }
}

#[derive(Clone)]
struct CorsConfig {
    allow_any: bool,
    origins: Vec<HeaderValue>,
}

impl CorsConfig {
    fn from_settings(settings: &RpcServerConfig, has_auth: bool) -> Option<Self> {
        if !settings.enable_cors {
            return None;
        }

        let allow_any = settings.allow_origins.is_empty();
        let mut invalid_origins = 0usize;
        let origins = settings
            .allow_origins
            .iter()
            .filter_map(|origin| {
                if let Ok(value) = HeaderValue::from_str(origin) {
                    Some(value)
                } else {
                    invalid_origins += 1;
                    None
                }
            })
            .collect::<Vec<_>>();

        if invalid_origins > 0 {
            tracing::warn!(
                invalid_origins,
                "Ignoring invalid CORS origin entries in allow_origins"
            );
        }
        if !allow_any && origins.is_empty() {
            tracing::warn!(
                "CORS is enabled but allow_origins contains no valid entries; CORS will be \
                 effectively disabled"
            );
        }

        if allow_any && has_auth {
            // We hard-error earlier in config validation, but keep the warning if ever bypassed.
            tracing::warn!(
                "SECURITY WARNING: CORS is configured to allow all origins ('*') while \
                authentication is enabled. This combination is insecure and may expose \
                your RPC server to CSRF attacks. Consider specifying explicit allowed \
                origins in the 'allow_origins' configuration."
            );
        }

        Some(Self { allow_any, origins })
    }

    fn origin_header(&self, request_origin: Option<&HeaderValue>) -> Option<HeaderValue> {
        if self.allow_any {
            Some(HeaderValue::from_static("*"))
        } else if let Some(origin) = request_origin {
            self.origins
                .iter()
                .find(|allowed| *allowed == origin)
                .cloned()
        } else {
            None
        }
    }
}

#[derive(Clone)]
struct RpcFilters {
    server: Weak<RwLock<RpcServer>>,
    disabled: Arc<HashSet<String>>,
    auth: Arc<Option<BasicAuth>>,
    rate_limiter: Option<Arc<GovernorRateLimiter>>,
    cors: Option<CorsConfig>,
}

#[derive(Debug, Default, Deserialize)]
struct RpcQueryParams {
    #[serde(default)]
    jsonrpc: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    params: Option<String>,
}

pub fn build_rpc_routes(
    handle: Weak<RwLock<RpcServer>>,
    disabled: Arc<HashSet<String>>,
    auth: Arc<Option<BasicAuth>>,
    settings: RpcServerConfig,
) -> impl Filter<Extract = (HttpResponse,), Error = warp::Rejection> + Clone {
    let has_auth = auth.is_some();
    let rate_limiter = if settings.max_requests_per_second > 0 {
        Some(Arc::new(GovernorRateLimiter::new(RateLimitConfig {
            max_rps: settings.max_requests_per_second,
            burst: if settings.rate_limit_burst == 0 {
                settings.max_requests_per_second
            } else {
                settings.rate_limit_burst
            },
        })))
    } else {
        None
    };
    let filters = RpcFilters {
        server: handle,
        disabled,
        auth,
        rate_limiter,
        cors: CorsConfig::from_settings(&settings, has_auth),
    };

    let max_body = settings.max_request_body_size as u64;
    let post_route = warp::path::end()
        .and(warp::post())
        .and(with_filters(filters.clone()))
        .and(warp::ext::optional::<SocketAddr>())
        .and(warp::header::optional::<HeaderValue>("origin"))
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::body::content_length_limit(max_body.max(1)))
        .and(warp::body::bytes())
        .and_then(handle_post_request);

    let max_query = settings.max_request_body_size as u64;
    let get_route = warp::path::end()
        .and(warp::get())
        .and(with_filters(filters.clone()))
        .and(warp::ext::optional::<SocketAddr>())
        .and(warp::header::optional::<HeaderValue>("origin"))
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::query::raw())
        .and_then(move |filters, remote, origin, auth, raw_query: String| {
            handle_get_request(filters, remote, origin, auth, raw_query, max_query)
        });

    let options_route = warp::path::end()
        .and(warp::options())
        .and(with_filters(filters))
        .and(warp::header::optional::<HeaderValue>("origin"))
        .map(|filters: RpcFilters, origin: Option<HeaderValue>| {
            let mut response = HttpResponse::new(Vec::new().into());
            *response.status_mut() = StatusCode::NO_CONTENT;
            apply_cors(&mut response, filters.cors.as_ref(), origin.as_ref());
            response
        });

    post_route.or(get_route).unify().or(options_route).unify()
}

/// Build WebSocket route for event subscriptions
///
/// This is separate from `build_rpc_routes` because WebSocket requires
/// an event broadcast channel which may not always be available.
pub fn build_ws_route(
    event_tx: tokio::sync::broadcast::Sender<super::ws::WsEvent>,
    subscription_mgr: std::sync::Arc<super::ws::SubscriptionManager>,
) -> impl Filter<Extract = (HttpResponse,), Error = warp::Rejection> + Clone {
    let event_tx = std::sync::Arc::new(event_tx);

    warp::path("ws")
        .and(warp::ws())
        .map(move |ws: warp::ws::Ws| {
            let event_rx = event_tx.subscribe();
            let mgr = subscription_mgr.clone();
            ws.on_upgrade(move |socket| super::ws::ws_handler(socket, event_rx, mgr))
        })
        .map(warp::reply::Reply::into_response)
}

fn with_filters(
    filters: RpcFilters,
) -> impl Filter<Extract = (RpcFilters,), Error = Infallible> + Clone {
    warp::any().map(move || filters.clone())
}

async fn handle_post_request(
    filters: RpcFilters,
    remote: Option<SocketAddr>,
    origin: Option<HeaderValue>,
    auth_header: Option<String>,
    body: Bytes,
) -> Result<HttpResponse, Infallible> {
    // Apply IP-based rate limiting first (before parsing body)
    // Use a default IP for requests where remote address is unavailable
    let client_ip = remote.map(|addr| addr.ip()).unwrap_or_else(|| {
        // Fallback to a dummy IP for rate limiting when remote is unavailable
        // This ensures rate limiting is always applied even if IP extraction fails
        "127.0.0.1".parse().unwrap()
    });

    if let Some(limiter) = filters.rate_limiter.as_ref() {
        let check_result = limiter.check(client_ip);
        if check_result.is_blocked() {
            let mut response = build_http_response(
                Some(error_response(None, RpcError::too_many_requests())),
                false,
                false,
            );
            apply_cors(&mut response, filters.cors.as_ref(), origin.as_ref());
            return Ok(response);
        }
    }

    // Process body and apply per-method rate limiting
    let (response, unauthorized) = process_body(
        &filters,
        auth_header.as_deref(),
        body.as_ref(),
        Some(client_ip),
    );

    let challenge = unauthorized && filters.auth.as_ref().is_some();
    let mut http_response = build_http_response(response, unauthorized, challenge);
    apply_cors(&mut http_response, filters.cors.as_ref(), origin.as_ref());
    Ok(http_response)
}

async fn handle_get_request(
    filters: RpcFilters,
    remote: Option<SocketAddr>,
    origin: Option<HeaderValue>,
    auth_header: Option<String>,
    raw_query: String,
    max_query_len: u64,
) -> Result<HttpResponse, Infallible> {
    // Apply IP-based rate limiting first (before parsing query)
    // Use a default IP for requests where remote address is unavailable
    let client_ip = remote.map(|addr| addr.ip()).unwrap_or_else(|| {
        // Fallback to a dummy IP for rate limiting when remote is unavailable
        // This ensures rate limiting is always applied even if IP extraction fails
        "127.0.0.1".parse().unwrap()
    });

    if let Some(limiter) = filters.rate_limiter.as_ref() {
        let check_result = limiter.check(client_ip);
        if check_result.is_blocked() {
            let mut response = build_http_response(
                Some(error_response(None, RpcError::too_many_requests())),
                false,
                false,
            );
            apply_cors(&mut response, filters.cors.as_ref(), origin.as_ref());
            return Ok(response);
        }
    }

    // Extract method from query for per-method rate limiting
    let method_from_query = query_to_request_value(&raw_query)
        .and_then(|v| v.get("method").and_then(|m| m.as_str()).map(String::from));

    // Apply per-method rate limiting if method is known
    if let (Some(limiter), Some(ref method)) = (filters.rate_limiter.as_ref(), method_from_query) {
        let check_result = limiter.check_for_method(client_ip, method);
        if check_result.is_blocked() {
            let mut response = build_http_response(
                Some(error_response(None, RpcError::too_many_requests())),
                false,
                false,
            );
            apply_cors(&mut response, filters.cors.as_ref(), origin.as_ref());
            return Ok(response);
        }
    }

    let (response, unauthorized) = if raw_query.len() as u64 > max_query_len {
        (Some(error_response(None, RpcError::bad_request())), false)
    } else {
        match query_to_request_value(&raw_query) {
            Some(value) if exceeds_max_depth(&value, MAX_PARAMS_DEPTH) => {
                (Some(error_response(None, RpcError::bad_request())), false)
            }
            Some(Value::Object(obj)) => {
                let outcome =
                    process_object(obj, &filters, auth_header.as_deref(), Some(client_ip));
                (outcome.response, outcome.unauthorized)
            }
            Some(_) => (
                Some(error_response(None, RpcError::invalid_request())),
                false,
            ),
            None => (
                Some(error_response(None, RpcError::invalid_request())),
                false,
            ),
        }
    };

    let challenge = unauthorized && filters.auth.as_ref().is_some();
    let mut http_response = build_http_response(response, unauthorized, challenge);
    apply_cors(&mut http_response, filters.cors.as_ref(), origin.as_ref());
    Ok(http_response)
}

fn process_body(
    filters: &RpcFilters,
    auth_header: Option<&str>,
    body: &[u8],
    client_ip: Option<IpAddr>,
) -> (Option<Value>, bool) {
    let parsed: Value = match serde_json::from_slice(body) {
        Ok(value) => value,
        Err(_) => return (Some(error_response(None, RpcError::bad_request())), false),
    };
    if exceeds_max_depth(&parsed, MAX_PARAMS_DEPTH) {
        return (Some(error_response(None, RpcError::bad_request())), false);
    }

    match parsed {
        Value::Array(entries) => process_array(entries, filters, auth_header, client_ip),
        Value::Object(obj) => {
            let outcome = process_object(obj, filters, auth_header, client_ip);
            (outcome.response, outcome.unauthorized)
        }
        _ => (
            Some(error_response(None, RpcError::invalid_request())),
            false,
        ),
    }
}

fn process_array(
    entries: Vec<Value>,
    filters: &RpcFilters,
    auth_header: Option<&str>,
    client_ip: Option<IpAddr>,
) -> (Option<Value>, bool) {
    if entries.is_empty() {
        return (
            Some(error_response(None, RpcError::invalid_request())),
            false,
        );
    }

    let mut responses = Vec::new();
    let mut unauthorized = false;
    for entry in entries {
        match entry {
            Value::Object(obj) => {
                let outcome = process_object(obj, filters, auth_header, client_ip);
                unauthorized |= outcome.unauthorized;
                if let Some(response) = outcome.response {
                    responses.push(response);
                }
            }
            _ => responses.push(error_response(None, RpcError::invalid_request())),
        }
    }

    if responses.is_empty() {
        (None, unauthorized)
    } else {
        (Some(Value::Array(responses)), unauthorized)
    }
}

fn process_object(
    mut obj: Map<String, Value>,
    filters: &RpcFilters,
    auth_header: Option<&str>,
    client_ip: Option<IpAddr>,
) -> RequestOutcome {
    RPC_REQ_TOTAL.inc();
    let has_id = obj.contains_key("id");
    let id = obj.get("id").cloned();

    if !has_id {
        return RequestOutcome::notification();
    }

    let method_value = obj.remove("method");
    let method = if let Some(value) =
        method_value.and_then(|value| value.as_str().map(std::string::ToString::to_string))
    {
        value
    } else {
        RPC_ERR_TOTAL.inc();
        return RequestOutcome::error(error_response(id, RpcError::invalid_request()), false);
    };

    // Apply per-method rate limiting if IP is available and rate limiter is configured
    if let (Some(limiter), Some(ip)) = (filters.rate_limiter.as_ref(), client_ip) {
        let check_result = limiter.check_for_method(ip, &method);
        if check_result.is_blocked() {
            RPC_ERR_TOTAL.inc();
            return RequestOutcome::error(error_response(id, RpcError::too_many_requests()), false);
        }
    }

    let params_value = obj.remove("params").unwrap_or(Value::Array(Vec::new()));
    let params = if let Value::Array(values) = params_value {
        values
    } else {
        RPC_ERR_TOTAL.inc();
        return RequestOutcome::error(error_response(id, RpcError::invalid_request()), false);
    };

    let method_key = method.to_ascii_lowercase();
    if filters.disabled.contains(&method_key) {
        RPC_ERR_TOTAL.inc();
        return RequestOutcome::error(error_response(id, RpcError::access_denied()), false);
    }

    let Some(server_arc) = filters.server.upgrade() else {
        RPC_ERR_TOTAL.inc();
        return RequestOutcome::error(error_response(id, RpcError::internal_server_error()), false);
    };

    let handler = {
        let server_guard = server_arc.read();
        let guard = server_guard.handlers_guard();
        guard.get(&method_key).cloned()
    };

    let Some(handler) = handler else {
        RPC_ERR_TOTAL.inc();
        return RequestOutcome::error(
            error_response(id, RpcError::method_not_found().with_data(method)),
            false,
        );
    };

    if let Some(auth) = filters.auth.as_ref() {
        let header = auth_header.unwrap_or("").trim();
        if header.is_empty() {
            RPC_ERR_TOTAL.inc();
            return RequestOutcome::error(error_response(id, RpcError::access_denied()), true);
        }
        if !verify_basic_auth(Some(header), auth) {
            RPC_ERR_TOTAL.inc();
            return RequestOutcome::error(error_response(id, RpcError::access_denied()), false);
        }
    }

    let policy = RpcServerSettings::current().exception_policy();
    let callback = handler.callback();
    let call_result = panic::catch_unwind(AssertUnwindSafe(|| {
        let server_guard = server_arc.read();
        (callback)(&server_guard, params.as_slice())
    }));

    match call_result {
        Ok(Ok(result)) => RequestOutcome::response(success_response(id, result)),
        Ok(Err(err)) => {
            RPC_ERR_TOTAL.inc();
            RequestOutcome::error(error_response(id, RpcError::from(err)), false)
        }
        Err(payload) => {
            RPC_ERR_TOTAL.inc();
            error!(
                target: "neo::rpc",
                method = method.as_str(),
                error = panic_message(&payload),
                "rpc handler panicked"
            );
            match policy {
                UnhandledExceptionPolicy::StopPlugin => {
                    let mut server = server_arc.write();
                    server.stop_rpc_server();
                }
                UnhandledExceptionPolicy::StopNode => std::process::exit(1),
                UnhandledExceptionPolicy::Terminate => std::process::abort(),
                UnhandledExceptionPolicy::Ignore
                | UnhandledExceptionPolicy::Log
                | UnhandledExceptionPolicy::Continue => {}
            }
            RequestOutcome::error(error_response(id, RpcError::internal_server_error()), false)
        }
    }
}

fn query_to_request_value(raw_query: &str) -> Option<Value> {
    let query: RpcQueryParams = serde_urlencoded::from_str(raw_query).ok()?;

    let method = query.method?;
    let id = query.id?;
    let params_raw = query.params?;
    let params_value = parse_query_params(&params_raw)?;

    let mut obj = Map::new();
    if let Some(jsonrpc) = query.jsonrpc {
        obj.insert("jsonrpc".to_string(), Value::String(jsonrpc));
    }
    obj.insert("id".to_string(), Value::String(id));
    obj.insert("method".to_string(), Value::String(method));
    obj.insert("params".to_string(), params_value);
    Some(Value::Object(obj))
}

fn parse_query_params(input: &str) -> Option<Value> {
    let decoded = BASE64_STANDARD
        .decode(input)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .or_else(|| serde_json::from_str::<Value>(input).ok())?;

    matches!(decoded, Value::Array(_)).then_some(decoded)
}

fn verify_basic_auth(header: Option<&str>, auth: &BasicAuth) -> bool {
    let header = match header {
        Some(value) => value.trim(),
        None => return false,
    };

    let mut parts = header.splitn(2, ' ');
    let scheme = parts.next().unwrap_or("");
    if !scheme.eq_ignore_ascii_case("basic") {
        return false;
    }

    let value = parts.next().unwrap_or("").trim();

    let decoded = match BASE64_STANDARD.decode(value) {
        Ok(bytes) => bytes,
        Err(_) => return false,
    };

    let Some(index) = decoded.iter().position(|byte| *byte == b':') else {
        return false;
    };

    let (user, pass) = decoded.split_at(index);
    let pass = &pass[1..];

    constant_time_equals(user, &auth.user) && constant_time_equals(pass, &auth.pass)
}

fn constant_time_equals(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len() && left.ct_eq(right).into()
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "panic".to_string()
    }
}

fn apply_cors(
    response: &mut HttpResponse,
    cors: Option<&CorsConfig>,
    request_origin: Option<&HeaderValue>,
) {
    if let Some(cors) = cors {
        if let Some(origin) = cors.origin_header(request_origin) {
            response
                .headers_mut()
                .insert(ACCESS_CONTROL_ALLOW_ORIGIN, origin);
        }
        if !cors.allow_any {
            response
                .headers_mut()
                .insert(VARY, HeaderValue::from_static("origin"));
            response.headers_mut().insert(
                ACCESS_CONTROL_ALLOW_CREDENTIALS,
                HeaderValue::from_static("true"),
            );
        }
        response.headers_mut().insert(
            ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("POST, GET, OPTIONS"),
        );
        response.headers_mut().insert(
            ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("content-type"),
        );
    }
}

fn success_response(id: Option<Value>, result: Value) -> Value {
    let mut response = Map::new();
    response.insert("jsonrpc".to_string(), Value::String("2.0".to_string()));
    response.insert("result".to_string(), result);
    response.insert("id".to_string(), id.unwrap_or(Value::Null));
    Value::Object(response)
}

fn error_response(id: Option<Value>, error: RpcError) -> Value {
    let mut response = Map::new();
    response.insert("jsonrpc".to_string(), Value::String("2.0".to_string()));
    response.insert("id".to_string(), id.unwrap_or(Value::Null));

    let mut error_obj = Map::new();
    error_obj.insert("code".to_string(), json!(error.code()));
    error_obj.insert("message".to_string(), Value::String(error.error_message()));
    if let Some(data) = error.data() {
        error_obj.insert("data".to_string(), Value::String(data.to_string()));
    }

    response.insert("error".to_string(), Value::Object(error_obj));
    Value::Object(response)
}

fn build_http_response(body: Option<Value>, unauthorized: bool, challenge: bool) -> HttpResponse {
    let (mut response, _has_body) = if let Some(body) = body {
        let json = serde_json::to_vec(&body).unwrap_or_else(|_| b"{}".to_vec());
        let mut response = HttpResponse::new(json.into());
        response
            .headers_mut()
            .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        (response, true)
    } else {
        (HttpResponse::new(Vec::new().into()), false)
    };

    *response.status_mut() = if unauthorized {
        StatusCode::UNAUTHORIZED
    } else {
        StatusCode::OK
    };

    if challenge {
        response.headers_mut().insert(
            WWW_AUTHENTICATE,
            HeaderValue::from_static("Basic realm=\"Restricted\""),
        );
    }

    response
}

struct RequestOutcome {
    response: Option<Value>,
    unauthorized: bool,
}

fn exceeds_max_depth(value: &Value, max_depth: usize) -> bool {
    fn walk(value: &Value, depth: usize, max_depth: usize) -> bool {
        if depth > max_depth {
            return true;
        }
        match value {
            Value::Array(values) => values.iter().any(|value| walk(value, depth + 1, max_depth)),
            Value::Object(map) => map.values().any(|value| walk(value, depth + 1, max_depth)),
            _ => false,
        }
    }

    walk(value, 1, max_depth)
}

impl RequestOutcome {
    const fn response(value: Value) -> Self {
        Self {
            response: Some(value),
            unauthorized: false,
        }
    }

    const fn error(value: Value, unauthorized: bool) -> Self {
        Self {
            response: Some(value),
            unauthorized,
        }
    }

    const fn notification() -> Self {
        Self {
            response: None,
            unauthorized: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::rcp_server_settings::RpcServerConfig;
    use crate::server::rpc_method_attribute::RpcMethodDescriptor;
    use crate::server::rpc_server::RpcHandler;
    use crate::server::rpc_server_blockchain::RpcServerBlockchain;
    use crate::server::rpc_server_node::RpcServerNode;
    use neo_core::neo_io::BinaryWriter;
    use neo_core::neo_system::NeoSystem;
    use neo_core::network::p2p::helper::get_sign_data_vec;
    use neo_core::network::p2p::payloads::signer::Signer;
    use neo_core::network::p2p::payloads::transaction::Transaction;
    use neo_core::network::p2p::payloads::witness::Witness;
    use neo_core::protocol_settings::ProtocolSettings;
    use neo_core::smart_contract::native::LedgerContract;
    use neo_core::smart_contract::{StorageItem, StorageKey};
    use neo_core::wallets::KeyPair;
    use neo_core::WitnessScope;
    use neo_vm::op_code::OpCode;
    use neo_vm::vm_state::VMState;
    use parking_lot::RwLock;
    use std::sync::Arc;

    fn build_test_routes(
        settings: RpcServerConfig,
    ) -> impl Filter<Extract = (HttpResponse,), Error = warp::Rejection> + Clone {
        let handle: Weak<RwLock<RpcServer>> = Weak::new();
        let disabled: Arc<HashSet<String>> = Arc::new(HashSet::new());
        let auth: Arc<Option<BasicAuth>> = Arc::new(None);
        build_rpc_routes(handle, disabled, auth, settings)
    }

    fn build_filters_with_handlers() -> (Arc<RwLock<RpcServer>>, RpcFilters) {
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let mut server = RpcServer::new(system, RpcServerConfig::default());
        server.register_handlers(RpcServerBlockchain::register_handlers());
        server.register_handlers(RpcServerNode::register_handlers());

        let server = Arc::new(RwLock::new(server));
        let filters = RpcFilters {
            server: Arc::downgrade(&server),
            disabled: Arc::new(HashSet::new()),
            auth: Arc::new(None),
            rate_limiter: None,
            cors: None,
        };
        (server, filters)
    }

    #[test]
    fn per_method_rate_limiting_blocks_expensive_methods() {
        use std::net::IpAddr;

        let config = RateLimitConfig {
            max_rps: 100,
            burst: 100,
        };
        let limiter = Arc::new(GovernorRateLimiter::new(config));
        let ip: IpAddr = "127.0.0.1".parse().unwrap();

        // Should categorize methods correctly
        assert_eq!(
            RateLimitTier::from_method("invokefunction"),
            RateLimitTier::Expensive
        );
        assert_eq!(
            RateLimitTier::from_method("sendrawtransaction"),
            RateLimitTier::Write
        );
        assert_eq!(
            RateLimitTier::from_method("getblockcount"),
            RateLimitTier::Cheap
        );

        // Expensive methods should have their own rate limit bucket
        let expensive_config = limiter.tier_config(RateLimitTier::Expensive).unwrap();
        assert!(expensive_config.max_rps < config.max_rps);

        // Test that rate limiting is enforced
        let result = limiter.check_for_method(ip, "invokefunction");
        assert!(result.is_allowed());
    }

    #[test]
    fn rate_limit_check_result_is_handled_properly() {
        // Verify that all check results must be explicitly handled
        let allowed = RateLimitCheckResult::Allowed;
        let blocked = RateLimitCheckResult::Blocked;
        let disabled = RateLimitCheckResult::Disabled;

        assert!(allowed.is_allowed());
        assert!(!allowed.is_blocked());

        assert!(!blocked.is_allowed());
        assert!(blocked.is_blocked());

        assert!(disabled.is_allowed());
        assert!(!disabled.is_blocked());
    }

    fn build_filters_with_panic_handler() -> (Arc<RwLock<RpcServer>>, RpcFilters) {
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let mut server = RpcServer::new(system, RpcServerConfig::default());
        server.register_handlers(RpcServerBlockchain::register_handlers());
        server.register_handlers(vec![RpcHandler::new(
            RpcMethodDescriptor::new("panic"),
            Arc::new(|_, _| panic!("boom")),
        )]);

        let server = Arc::new(RwLock::new(server));
        let filters = RpcFilters {
            server: Arc::downgrade(&server),
            disabled: Arc::new(HashSet::new()),
            auth: Arc::new(None),
            rate_limiter: None,
            cors: None,
        };
        (server, filters)
    }

    fn build_filters_with_auth(
        auth: Arc<Option<BasicAuth>>,
        include_wallet: bool,
    ) -> (Arc<RwLock<RpcServer>>, RpcFilters) {
        let system =
            NeoSystem::new(ProtocolSettings::default(), None, None).expect("system to start");
        let mut server = RpcServer::new(system, RpcServerConfig::default());
        server.register_handlers(RpcServerBlockchain::register_handlers());
        server.register_handlers(RpcServerNode::register_handlers());
        if include_wallet {
            server.register_handlers(
                crate::server::rpc_server_wallet::RpcServerWallet::register_handlers(),
            );
        }

        let server = Arc::new(RwLock::new(server));
        let filters = RpcFilters {
            server: Arc::downgrade(&server),
            disabled: Arc::new(HashSet::new()),
            auth,
            rate_limiter: None,
            cors: None,
        };
        (server, filters)
    }

    #[test]
    fn verify_basic_auth_accepts_valid_credentials() {
        let auth = BasicAuth {
            user: b"testuser".to_vec(),
            pass: b"testpass".to_vec(),
        };
        let header = format!("Basic {}", BASE64_STANDARD.encode("testuser:testpass"));
        assert!(verify_basic_auth(Some(&header), &auth));
    }

    #[test]
    fn verify_basic_auth_rejects_invalid_credentials() {
        let auth = BasicAuth {
            user: b"testuser".to_vec(),
            pass: b"testpass".to_vec(),
        };
        let wrong_user = format!("Basic {}", BASE64_STANDARD.encode("wrong:testpass"));
        let wrong_pass = format!("Basic {}", BASE64_STANDARD.encode("testuser:wrong"));
        let wrong_scheme = format!("Bearer {}", BASE64_STANDARD.encode("testuser:testpass"));

        assert!(!verify_basic_auth(Some(&wrong_user), &auth));
        assert!(!verify_basic_auth(Some(&wrong_pass), &auth));
        assert!(!verify_basic_auth(Some(&wrong_scheme), &auth));
        assert!(!verify_basic_auth(None, &auth));
    }

    #[tokio::test]
    async fn cors_echoes_matching_origin_from_allowlist() {
        let settings = RpcServerConfig {
            enable_cors: true,
            allow_origins: vec!["https://a.example".into(), "https://b.example".into()],
            ..Default::default()
        };

        let routes = build_test_routes(settings);

        let resp = warp::test::request()
            .method("OPTIONS")
            .path("/")
            .header("origin", "https://b.example")
            .reply(&routes)
            .await;

        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            resp.headers()
                .get(ACCESS_CONTROL_ALLOW_ORIGIN)
                .and_then(|v| v.to_str().ok()),
            Some("https://b.example")
        );
        assert_eq!(
            resp.headers().get(VARY).and_then(|v| v.to_str().ok()),
            Some("origin")
        );
    }

    #[tokio::test]
    async fn cors_omits_allow_origin_for_disallowed_origin() {
        let settings = RpcServerConfig {
            enable_cors: true,
            allow_origins: vec!["https://a.example".into()],
            ..Default::default()
        };

        let routes = build_test_routes(settings);

        let resp = warp::test::request()
            .method("OPTIONS")
            .path("/")
            .header("origin", "https://b.example")
            .reply(&routes)
            .await;

        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert!(resp.headers().get(ACCESS_CONTROL_ALLOW_ORIGIN).is_none());
        assert_eq!(
            resp.headers().get(VARY).and_then(|v| v.to_str().ok()),
            Some("origin")
        );
    }

    #[tokio::test]
    async fn cors_wildcard_allows_any_origin() {
        let settings = RpcServerConfig {
            enable_cors: true,
            allow_origins: Vec::new(), // wildcard
            ..Default::default()
        };

        let routes = build_test_routes(settings);

        let resp = warp::test::request()
            .method("OPTIONS")
            .path("/")
            .header("origin", "https://whatever.example")
            .reply(&routes)
            .await;

        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            resp.headers()
                .get(ACCESS_CONTROL_ALLOW_ORIGIN)
                .and_then(|v| v.to_str().ok()),
            Some("*")
        );
        assert!(resp.headers().get(VARY).is_none());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn process_body_rejects_malformed_json() {
        let (_server, filters) = build_filters_with_handlers();
        let (response, unauthorized) = process_body(&filters, None, b"{ invalid json");
        assert!(!unauthorized);

        let response = response.expect("response");
        let error = response
            .get("error")
            .and_then(Value::as_object)
            .expect("error object");
        let code = error.get("code").and_then(Value::as_i64).expect("code");
        assert_eq!(code, RpcError::bad_request().code() as i64);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn process_body_rejects_empty_batch() {
        let (_server, filters) = build_filters_with_handlers();
        let (response, unauthorized) = process_body(&filters, None, b"[]");
        assert!(!unauthorized);

        let response = response.expect("response");
        let error = response
            .get("error")
            .and_then(Value::as_object)
            .expect("error object");
        let code = error.get("code").and_then(Value::as_i64).expect("code");
        assert_eq!(code, RpcError::invalid_request().code() as i64);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn process_body_mixed_batch() {
        let (_server, filters) = build_filters_with_handlers();
        let body = br#"[
            {"jsonrpc": "2.0", "method": "getblockcount", "params": [], "id": 1},
            {"jsonrpc": "2.0", "method": "nonexistentmethod", "params": [], "id": 2},
            {"jsonrpc": "2.0", "method": "getblock", "params": ["invalid_index"], "id": 3},
            {"jsonrpc": "2.0", "method": "getversion", "id": 4}
        ]"#;

        let (response, unauthorized) = process_body(&filters, None, body);
        assert!(!unauthorized);

        let response = response.expect("response");
        let batch = response.as_array().expect("batch array");
        assert_eq!(batch.len(), 4);

        let first = batch[0].as_object().expect("first response");
        assert!(first.get("error").is_none());
        assert!(first.get("result").is_some());
        assert_eq!(first.get("id").and_then(Value::as_i64), Some(1));

        let second = batch[1].as_object().expect("second response");
        let second_error = second
            .get("error")
            .and_then(Value::as_object)
            .expect("second error");
        assert_eq!(
            second_error.get("code").and_then(Value::as_i64),
            Some(RpcError::method_not_found().code() as i64)
        );
        assert_eq!(second.get("id").and_then(Value::as_i64), Some(2));

        let third = batch[2].as_object().expect("third response");
        let third_error = third
            .get("error")
            .and_then(Value::as_object)
            .expect("third error");
        assert_eq!(
            third_error.get("code").and_then(Value::as_i64),
            Some(RpcError::invalid_params().code() as i64)
        );
        assert_eq!(third.get("id").and_then(Value::as_i64), Some(3));

        let fourth = batch[3].as_object().expect("fourth response");
        assert!(fourth.get("error").is_none());
        assert!(fourth.get("result").is_some());
        assert_eq!(fourth.get("id").and_then(Value::as_i64), Some(4));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn process_body_reports_already_exists_for_sendrawtransaction() {
        let (server, filters) = build_filters_with_handlers();
        let settings = ProtocolSettings::default();
        let keypair = KeyPair::from_private_key(&[0x55u8; 32]).expect("keypair");
        let tx = build_signed_transaction(&settings, &keypair, 2, 0);
        let mut store = server.read().system().context().store_snapshot_cache();
        persist_transaction_record(&mut store, &tx, 1);

        let payload = BASE64_STANDARD.encode(tx.to_bytes());
        let request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "sendrawtransaction",
            "params": [payload],
        });
        let body = serde_json::to_vec(&request).expect("serialize body");
        let (response, unauthorized) =
            tokio::task::block_in_place(|| process_body(&filters, None, &body));
        assert!(!unauthorized);

        let response = response.expect("response");
        let error = response
            .get("error")
            .and_then(Value::as_object)
            .expect("error object");
        assert_eq!(
            error.get("code").and_then(Value::as_i64),
            Some(RpcError::already_exists().code() as i64)
        );
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert!(message.contains(RpcError::already_exists().message()));
    }

    fn build_signed_transaction(
        settings: &ProtocolSettings,
        keypair: &KeyPair,
        nonce: u32,
        system_fee: i64,
    ) -> Transaction {
        let mut tx = Transaction::new();
        tx.set_nonce(nonce);
        tx.set_network_fee(1_0000_0000);
        tx.set_system_fee(system_fee);
        tx.set_valid_until_block(1);
        tx.set_script(vec![OpCode::PUSH1 as u8]);
        tx.set_signers(vec![Signer::new(
            keypair.get_script_hash(),
            WitnessScope::GLOBAL,
        )]);

        let sign_data = get_sign_data_vec(&tx, settings.network).expect("sign data");
        let signature = keypair.sign(&sign_data).expect("sign");
        let mut invocation = Vec::with_capacity(signature.len() + 2);
        invocation.push(OpCode::PUSHDATA1 as u8);
        invocation.push(signature.len() as u8);
        invocation.extend_from_slice(&signature);
        let verification_script = keypair.get_verification_script();
        tx.set_witnesses(vec![Witness::new_with_scripts(
            invocation,
            verification_script,
        )]);
        tx
    }

    fn persist_transaction_record(
        store: &mut neo_core::persistence::StoreCache,
        tx: &Transaction,
        block_index: u32,
    ) {
        const PREFIX_TRANSACTION: u8 = 0x0b;
        const RECORD_KIND_TRANSACTION: u8 = 0x01;

        let mut writer = BinaryWriter::new();
        writer
            .write_u8(RECORD_KIND_TRANSACTION)
            .expect("record kind");
        writer.write_u32(block_index).expect("block index");
        writer.write_u8(VMState::NONE as u8).expect("vm state");
        let tx_bytes = tx.to_bytes();
        writer.write_var_bytes(&tx_bytes).expect("tx bytes");

        let mut key_bytes = Vec::with_capacity(1 + 32);
        key_bytes.push(PREFIX_TRANSACTION);
        key_bytes.extend_from_slice(&tx.hash().to_bytes());
        let key = StorageKey::new(LedgerContract::ID, key_bytes);
        store.add(key, StorageItem::from_bytes(writer.to_bytes()));
        store.commit();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn process_body_allows_wallet_method_without_auth_config() {
        let (_server, filters) = build_filters_with_auth(Arc::new(None), true);
        let body = br#"{"jsonrpc": "2.0", "method": "getnewaddress", "params": [], "id": 1}"#;

        let (response, unauthorized) = process_body(&filters, None, body);
        assert!(!unauthorized);

        let response = response.expect("response");
        let error = response
            .get("error")
            .and_then(Value::as_object)
            .expect("error object");
        assert_eq!(
            error.get("code").and_then(Value::as_i64),
            Some(RpcError::no_opened_wallet().code() as i64)
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn process_body_rejects_invalid_auth_header() {
        let auth = Arc::new(Some(BasicAuth {
            user: b"testuser".to_vec(),
            pass: b"testpass".to_vec(),
        }));
        let (_server, filters) = build_filters_with_auth(auth, false);
        let body = br#"{"jsonrpc": "2.0", "method": "getblockcount", "params": [], "id": 1}"#;

        let header = format!("Basic {}", BASE64_STANDARD.encode("testuser:wrongpass"));
        let (response, unauthorized) = process_body(&filters, Some(&header), body);
        assert!(!unauthorized);

        let response = response.expect("response");
        let error = response
            .get("error")
            .and_then(Value::as_object)
            .expect("error object");
        assert_eq!(
            error.get("code").and_then(Value::as_i64),
            Some(RpcError::access_denied().code() as i64)
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn process_body_rejects_missing_auth_header() {
        let auth = Arc::new(Some(BasicAuth {
            user: b"testuser".to_vec(),
            pass: b"testpass".to_vec(),
        }));
        let (_server, filters) = build_filters_with_auth(auth, false);
        let body = br#"{"jsonrpc": "2.0", "method": "getblockcount", "params": [], "id": 1}"#;

        let (response, unauthorized) = process_body(&filters, None, body);
        assert!(unauthorized);

        let response = response.expect("response");
        let error = response
            .get("error")
            .and_then(Value::as_object)
            .expect("error object");
        assert_eq!(
            error.get("code").and_then(Value::as_i64),
            Some(RpcError::access_denied().code() as i64)
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn process_body_accepts_valid_auth_header() {
        let auth = Arc::new(Some(BasicAuth {
            user: b"testuser".to_vec(),
            pass: b"testpass".to_vec(),
        }));
        let (_server, filters) = build_filters_with_auth(auth, false);
        let body = br#"{"jsonrpc": "2.0", "method": "getblockcount", "params": [], "id": 1}"#;

        let header = format!("Basic {}", BASE64_STANDARD.encode("testuser:testpass"));
        let (response, unauthorized) = process_body(&filters, Some(&header), body);
        assert!(!unauthorized);

        let response = response.expect("response");
        let obj = response.as_object().expect("response object");
        assert!(obj.get("error").is_none());
        assert!(obj.get("result").is_some());
        assert_eq!(obj.get("id").and_then(Value::as_i64), Some(1));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn process_body_returns_internal_error_on_panic() {
        let (_server, filters) = build_filters_with_panic_handler();
        let body = br#"{"jsonrpc": "2.0", "method": "panic", "params": [], "id": 1}"#;

        let (response, unauthorized) = process_body(&filters, None, body);
        assert!(!unauthorized);

        let response = response.expect("response");
        let error = response
            .get("error")
            .and_then(Value::as_object)
            .expect("error object");
        assert_eq!(
            error.get("code").and_then(Value::as_i64),
            Some(RpcError::internal_server_error().code() as i64)
        );
    }
}
