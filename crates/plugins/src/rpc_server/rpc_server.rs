use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use bytes::Bytes;
use neo_core::{neo_system::NeoSystem, wallets::Wallet};
use once_cell::sync::Lazy;
use parking_lot::{RwLock, RwLockReadGuard};
use prometheus::{register_counter, Counter};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use subtle::ConstantTimeEq;
use tokio::{
    sync::{oneshot, OwnedSemaphorePermit, Semaphore},
    task::JoinHandle,
};
use tracing::{error, info, warn};
use uuid::Uuid;
use warp::http::header::{
    HeaderValue, ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS,
    ACCESS_CONTROL_ALLOW_ORIGIN, CONTENT_TYPE, WWW_AUTHENTICATE,
};
use warp::http::StatusCode;
use warp::reply::Response as HttpResponse;
use warp::Filter;

use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Weak};
use std::time::Duration;

use super::rcp_server_settings::RpcServerConfig;
use super::session::Session;
use crate::rpc_server::rpc_error::RpcError;
use crate::rpc_server::rpc_exception::RpcException;
use crate::rpc_server::rpc_method_attribute::RpcMethodDescriptor;

pub type RpcCallback =
    dyn Fn(&RpcServer, &[Value]) -> Result<Value, RpcException> + Send + Sync + 'static;

pub struct RpcHandler {
    descriptor: RpcMethodDescriptor,
    callback: Arc<RpcCallback>,
}

impl RpcHandler {
    pub fn new(descriptor: RpcMethodDescriptor, callback: Arc<RpcCallback>) -> Self {
        Self {
            descriptor,
            callback,
        }
    }

    pub fn descriptor(&self) -> &RpcMethodDescriptor {
        &self.descriptor
    }

    pub fn callback(&self) -> Arc<RpcCallback> {
        Arc::clone(&self.callback)
    }
}

pub static RPC_REQ_TOTAL: Lazy<Counter> =
    Lazy::new(|| register_counter!("neo_rpc_requests_total", "Total RPC requests").unwrap());
pub static RPC_ERR_TOTAL: Lazy<Counter> =
    Lazy::new(|| register_counter!("neo_rpc_errors_total", "Total RPC errors").unwrap());

pub struct RpcServer {
    system: Arc<NeoSystem>,
    settings: RpcServerConfig,
    handler_lookup: Arc<RwLock<HashMap<String, Arc<RpcHandler>>>>,
    started: bool,
    wallet: Arc<RwLock<Option<Arc<dyn Wallet>>>>,
    sessions: Arc<RwLock<HashMap<Uuid, Session>>>,
    server_task: Option<JoinHandle<()>>,
    shutdown_signal: Option<oneshot::Sender<()>>,
    self_handle: Option<Weak<RwLock<RpcServer>>>,
}

impl RpcServer {
    pub fn new(system: Arc<NeoSystem>, settings: RpcServerConfig) -> Self {
        Self {
            system,
            settings,
            handler_lookup: Arc::new(RwLock::new(HashMap::new())),
            started: false,
            wallet: Arc::new(RwLock::new(None)),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            server_task: None,
            shutdown_signal: None,
            self_handle: None,
        }
    }

    pub fn settings(&self) -> &RpcServerConfig {
        &self.settings
    }

    pub fn update_settings(&mut self, settings: RpcServerConfig) {
        self.settings = settings;
    }

    pub fn system(&self) -> Arc<NeoSystem> {
        Arc::clone(&self.system)
    }

    pub fn start_rpc_server(&mut self, handle: Weak<RwLock<RpcServer>>) {
        if self.started {
            return;
        }

        self.self_handle = Some(handle.clone());

        if !self.settings.ssl_cert.is_empty() {
            warn!("RPC TLS certificates are not supported yet; continuing without SSL binding");
        }

        if !self.settings.trusted_authorities.is_empty() {
            warn!("RPC client certificate validation is not supported yet");
        }

        let disabled_methods: Arc<HashSet<String>> = Arc::new(
            self.settings
                .disabled_methods
                .iter()
                .map(|name| name.to_ascii_lowercase())
                .collect(),
        );
        let auth = Arc::new(BasicAuth::from_settings(&self.settings));
        let semaphore = Arc::new(Semaphore::new(
            self.settings.max_concurrent_connections.max(1),
        ));
        let address = SocketAddr::new(self.settings.bind_address, self.settings.port);

        let routes = build_rpc_routes(
            handle,
            disabled_methods,
            auth.clone(),
            semaphore,
            self.settings.clone(),
        );

        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let (bound_addr, server) =
            warp::serve(routes).bind_with_graceful_shutdown(address, async move {
                let _ = shutdown_rx.await;
            });

        info!("RPC server bound on {}", bound_addr);
        let task = tokio::spawn(async move {
            server.await;
        });

        self.shutdown_signal = Some(shutdown_tx);
        self.server_task = Some(task);
        self.started = true;
        info!(
            "Starting RPC server on {}:{} (network {})",
            self.settings.bind_address, self.settings.port, self.settings.network
        );
    }

    pub fn stop_rpc_server(&mut self) {
        if !self.started {
            return;
        }

        if let Some(tx) = self.shutdown_signal.take() {
            let _ = tx.send(());
        }

        if let Some(handle) = self.server_task.take() {
            tokio::spawn(async move {
                if let Err(err) = handle.await {
                    log_join_error(err);
                }
            });
        }

        info!("Stopping RPC server for network {}", self.settings.network);
        self.started = false;
    }

    pub fn register_method(&mut self, handler: RpcHandler) {
        let key = handler.descriptor().name.to_ascii_lowercase();
        self.handler_lookup.write().insert(key, Arc::new(handler));
    }

    pub fn register_handlers(&mut self, handlers: Vec<RpcHandler>) {
        for handler in handlers {
            self.register_method(handler);
        }
    }

    pub fn is_started(&self) -> bool {
        self.started
    }

    pub fn dispose(&mut self) {
        self.stop_rpc_server();
        self.handler_lookup.write().clear();
        self.set_wallet(None);
        self.sessions.write().clear();
    }

    pub fn set_wallet(&self, wallet: Option<Arc<dyn Wallet>>) {
        *self.wallet.write() = wallet;
    }

    pub fn wallet(&self) -> Option<Arc<dyn Wallet>> {
        self.wallet.read().clone()
    }

    fn session_expiration(&self) -> Duration {
        Duration::from_secs(self.settings.session_expiration_time)
    }

    pub fn session_enabled(&self) -> bool {
        self.settings.session_enabled
    }

    pub fn purge_expired_sessions(&self) {
        if !self.session_enabled() {
            return;
        }
        let expiration = self.session_expiration();
        let mut guard = self.sessions.write();
        guard.retain(|_, session| !session.is_expired(expiration));
    }

    pub fn store_session(&self, session: Session) -> Uuid {
        let id = Uuid::new_v4();
        self.sessions.write().insert(id, session);
        id
    }

    pub fn with_session_mut<F, R>(&self, id: &Uuid, func: F) -> Option<R>
    where
        F: FnOnce(&mut Session) -> R,
    {
        let mut guard = self.sessions.write();
        guard.get_mut(id).map(func)
    }

    pub fn terminate_session(&self, id: &Uuid) -> bool {
        self.sessions.write().remove(id).is_some()
    }

    fn handlers_guard(&self) -> RwLockReadGuard<'_, HashMap<String, Arc<RpcHandler>>> {
        self.handler_lookup.read()
    }
}

#[derive(Clone)]
struct BasicAuth {
    user: Vec<u8>,
    pass: Vec<u8>,
}

impl BasicAuth {
    fn from_settings(settings: &RpcServerConfig) -> Option<Self> {
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
    fn from_settings(settings: &RpcServerConfig) -> Option<Self> {
        if !settings.enable_cors {
            return None;
        }

        let origins = settings
            .allow_origins
            .iter()
            .filter_map(|origin| HeaderValue::from_str(origin).ok())
            .collect::<Vec<_>>();

        Some(Self {
            allow_any: settings.allow_origins.is_empty(),
            origins,
        })
    }

    fn origin_header(&self) -> Option<HeaderValue> {
        if self.allow_any {
            Some(HeaderValue::from_static("*"))
        } else {
            self.origins.first().cloned()
        }
    }
}

#[derive(Clone)]
struct RpcFilters {
    server: Weak<RwLock<RpcServer>>,
    disabled: Arc<HashSet<String>>,
    auth: Arc<Option<BasicAuth>>,
    semaphore: Arc<Semaphore>,
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

fn build_rpc_routes(
    handle: Weak<RwLock<RpcServer>>,
    disabled: Arc<HashSet<String>>,
    auth: Arc<Option<BasicAuth>>,
    semaphore: Arc<Semaphore>,
    settings: RpcServerConfig,
) -> impl Filter<Extract = (HttpResponse,), Error = warp::Rejection> + Clone {
    let filters = RpcFilters {
        server: handle,
        disabled,
        auth,
        semaphore,
        cors: CorsConfig::from_settings(&settings),
    };

    let max_body = settings.max_request_body_size as u64;
    let post_route = warp::path::end()
        .and(warp::post())
        .and(with_filters(filters.clone()))
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::body::content_length_limit(max_body.max(1)))
        .and(warp::body::bytes())
        .and_then(handle_post_request);

    let get_route = warp::path::end()
        .and(warp::get())
        .and(with_filters(filters.clone()))
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::query::<RpcQueryParams>())
        .and_then(handle_get_request);

    let options_route = warp::path::end()
        .and(warp::options())
        .and(with_filters(filters.clone()))
        .map(|filters: RpcFilters| {
            let mut response = HttpResponse::new(Vec::new().into());
            *response.status_mut() = StatusCode::NO_CONTENT;
            apply_cors(&mut response, filters.cors.as_ref());
            response
        });

    post_route.or(get_route).unify().or(options_route).unify()
}

fn with_filters(
    filters: RpcFilters,
) -> impl Filter<Extract = (RpcFilters,), Error = Infallible> + Clone {
    warp::any().map(move || filters.clone())
}

async fn handle_post_request(
    filters: RpcFilters,
    auth_header: Option<String>,
    body: Bytes,
) -> Result<HttpResponse, Infallible> {
    let permit = acquire_permit(filters.semaphore.clone()).await;
    let (response, unauthorized) = if permit.is_some() {
        process_body(&filters, auth_header.as_deref(), body.as_ref())
    } else {
        (
            Some(error_response(None, RpcError::internal_server_error())),
            false,
        )
    };
    drop(permit);

    let challenge = unauthorized && filters.auth.as_ref().is_some();
    let mut http_response = build_http_response(response, unauthorized, challenge);
    apply_cors(&mut http_response, filters.cors.as_ref());
    Ok(http_response)
}

async fn handle_get_request(
    filters: RpcFilters,
    auth_header: Option<String>,
    query: RpcQueryParams,
) -> Result<HttpResponse, Infallible> {
    let permit = acquire_permit(filters.semaphore.clone()).await;
    let (response, unauthorized) = if permit.is_some() {
        match query_to_request_value(&query) {
            Some(Value::Object(obj)) => {
                let outcome = process_object(obj, &filters, auth_header.as_deref());
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
    } else {
        (
            Some(error_response(None, RpcError::internal_server_error())),
            false,
        )
    };
    drop(permit);

    let challenge = unauthorized && filters.auth.as_ref().is_some();
    let mut http_response = build_http_response(response, unauthorized, challenge);
    apply_cors(&mut http_response, filters.cors.as_ref());
    Ok(http_response)
}

async fn acquire_permit(semaphore: Arc<Semaphore>) -> Option<OwnedSemaphorePermit> {
    match semaphore.acquire_owned().await {
        Ok(permit) => Some(permit),
        Err(_) => None,
    }
}

fn process_body(
    filters: &RpcFilters,
    auth_header: Option<&str>,
    body: &[u8],
) -> (Option<Value>, bool) {
    let parsed: Value = match serde_json::from_slice(body) {
        Ok(value) => value,
        Err(_) => return (Some(error_response(None, RpcError::bad_request())), false),
    };

    match parsed {
        Value::Array(entries) => process_array(entries, filters, auth_header),
        Value::Object(obj) => {
            let outcome = process_object(obj, filters, auth_header);
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
                let outcome = process_object(obj, filters, auth_header);
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
) -> RequestOutcome {
    RPC_REQ_TOTAL.inc();
    let has_id = obj.contains_key("id");
    let id = obj.get("id").cloned();

    if !has_id {
        return RequestOutcome::notification();
    }

    let method_value = obj.remove("method");
    let method = match method_value.and_then(|value| value.as_str().map(|s| s.to_string())) {
        Some(value) => value,
        None => {
            RPC_ERR_TOTAL.inc();
            return RequestOutcome::error(error_response(id, RpcError::invalid_request()), false);
        }
    };

    let params_value = obj.remove("params").unwrap_or(Value::Array(Vec::new()));
    let params = match params_value {
        Value::Array(values) => values,
        _ => {
            RPC_ERR_TOTAL.inc();
            return RequestOutcome::error(error_response(id, RpcError::invalid_request()), false);
        }
    };

    if let Some(auth) = filters.auth.as_ref() {
        if !verify_basic_auth(auth_header, auth) {
            RPC_ERR_TOTAL.inc();
            return RequestOutcome::error(error_response(id, RpcError::access_denied()), true);
        }
    }

    let method_key = method.to_ascii_lowercase();
    if filters.disabled.contains(&method_key) {
        RPC_ERR_TOTAL.inc();
        return RequestOutcome::error(error_response(id, RpcError::access_denied()), false);
    }

    let Some(server_arc) = filters.server.upgrade() else {
        RPC_ERR_TOTAL.inc();
        return RequestOutcome::error(error_response(id, RpcError::internal_server_error()), false);
    };

    let server_guard = server_arc.read();
    let handler = {
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

    match handler.callback()(&server_guard, params.as_slice()) {
        Ok(result) => RequestOutcome::response(success_response(id, result)),
        Err(err) => {
            RPC_ERR_TOTAL.inc();
            RequestOutcome::error(error_response(id, err.error().clone()), false)
        }
    }
}

fn query_to_request_value(query: &RpcQueryParams) -> Option<Value> {
    let method = query.method.clone()?;
    let id = query.id.clone()?;
    let params_raw = query.params.clone()?;
    let params_value = parse_query_params(&params_raw)?;

    let mut obj = Map::new();
    if let Some(jsonrpc) = &query.jsonrpc {
        obj.insert("jsonrpc".to_string(), Value::String(jsonrpc.clone()));
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

fn apply_cors(response: &mut HttpResponse, cors: Option<&CorsConfig>) {
    if let Some(cors) = cors {
        if let Some(origin) = cors.origin_header() {
            response
                .headers_mut()
                .insert(ACCESS_CONTROL_ALLOW_ORIGIN, origin);
        }
        response.headers_mut().insert(
            ACCESS_CONTROL_ALLOW_METHODS,
            HeaderValue::from_static("POST, GET, OPTIONS"),
        );
        response.headers_mut().insert(
            ACCESS_CONTROL_ALLOW_HEADERS,
            HeaderValue::from_static("content-type, authorization"),
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
    let (mut response, has_body) = if let Some(body) = body {
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
    } else if has_body {
        StatusCode::OK
    } else {
        StatusCode::NO_CONTENT
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

impl RequestOutcome {
    fn response(value: Value) -> Self {
        Self {
            response: Some(value),
            unauthorized: false,
        }
    }

    fn error(value: Value, unauthorized: bool) -> Self {
        Self {
            response: Some(value),
            unauthorized,
        }
    }

    fn notification() -> Self {
        Self {
            response: None,
            unauthorized: false,
        }
    }
}

fn log_join_error(error: tokio::task::JoinError) {
    if error.is_cancelled() {
        warn!(target: "neo", "rpc server task cancelled before completion");
    } else {
        match error.try_into_panic() {
            Ok(payload) => {
                if let Some(message) = payload.downcast_ref::<&str>() {
                    error!(target: "neo", message = %message, "rpc server panicked");
                } else if let Some(message) = payload.downcast_ref::<String>() {
                    error!(target: "neo", message = %message, "rpc server panicked");
                } else {
                    error!(target: "neo", "rpc server panicked");
                }
            }
            Err(join_err) => {
                error!(target: "neo", error = %join_err, "rpc server task failed");
            }
        }
    }
}

pub static SERVERS: Lazy<RwLock<HashMap<u32, Arc<RwLock<RpcServer>>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

pub static PENDING_HANDLERS: Lazy<RwLock<HashMap<u32, Vec<RpcHandler>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

pub fn remove_server(network: u32) {
    if SERVERS.write().remove(&network).is_some() {
        info!("Removed RPC server for network {}", network);
    }
}

pub fn add_pending_handler(network: u32, handler: RpcHandler) {
    let mut guard = PENDING_HANDLERS.write();
    guard.entry(network).or_default().push(handler);
}

pub fn take_pending_handlers(network: u32) -> Vec<RpcHandler> {
    PENDING_HANDLERS
        .write()
        .remove(&network)
        .unwrap_or_default()
}

pub fn register_server(network: u32, server: Arc<RwLock<RpcServer>>) {
    let mut guard = SERVERS.write();
    if let Some(previous) = guard.insert(network, Arc::clone(&server)) {
        warn!(
            "Replacing existing RPC server instance for network {}",
            network
        );
        if let Some(mut previous_guard) = previous.try_write() {
            previous_guard.dispose();
        }
    }
}

pub fn get_server(network: u32) -> Option<Arc<RwLock<RpcServer>>> {
    SERVERS.read().get(&network).cloned()
}
