use super::rcp_server_settings::RpcServerConfig;
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
use std::sync::{Arc, Weak};
use subtle::ConstantTimeEq;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use warp::http::header::{
    HeaderValue, ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS,
    ACCESS_CONTROL_ALLOW_ORIGIN, CONTENT_TYPE, WWW_AUTHENTICATE,
};
use warp::http::StatusCode;
use warp::reply::Response as HttpResponse;
use warp::Filter;

#[derive(Clone)]
pub(crate) struct BasicAuth {
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

        let origins = settings
            .allow_origins
            .iter()
            .filter_map(|origin| HeaderValue::from_str(origin).ok())
            .collect::<Vec<_>>();

        let allow_any = settings.allow_origins.is_empty();

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

pub(crate) fn build_rpc_routes(
    handle: Weak<RwLock<RpcServer>>,
    disabled: Arc<HashSet<String>>,
    auth: Arc<Option<BasicAuth>>,
    semaphore: Arc<Semaphore>,
    settings: RpcServerConfig,
) -> impl Filter<Extract = (HttpResponse,), Error = warp::Rejection> + Clone {
    let has_auth = auth.is_some();
    let filters = RpcFilters {
        server: handle,
        disabled,
        auth,
        semaphore,
        cors: CorsConfig::from_settings(&settings, has_auth),
    };

    let max_body = settings.max_request_body_size as u64;
    let post_route = warp::path::end()
        .and(warp::post())
        .and(with_filters(filters.clone()))
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::body::content_length_limit(max_body.max(1)))
        .and(warp::body::bytes())
        .and_then(handle_post_request);

    let max_query = settings.max_request_body_size as u64;
    let get_route = warp::path::end()
        .and(warp::get())
        .and(with_filters(filters.clone()))
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::query::raw())
        .and_then(move |filters, auth, raw_query: String| {
            handle_get_request(filters, auth, raw_query, max_query)
        });

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
    raw_query: String,
    max_query_len: u64,
) -> Result<HttpResponse, Infallible> {
    let permit = acquire_permit(filters.semaphore.clone()).await;
    let (response, unauthorized) = if permit.is_some() {
        if raw_query.len() as u64 > max_query_len {
            (Some(error_response(None, RpcError::bad_request())), false)
        } else {
            match query_to_request_value(&raw_query) {
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
    semaphore.acquire_owned().await.ok()
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

    // SECURITY FIX: Check authentication requirements
    // Protected methods (like wallet operations) require authentication even if
    // global auth is not configured. This prevents sensitive operations from
    // being exposed without authentication.
    let requires_auth = handler.descriptor().requires_auth();
    if let Some(auth) = filters.auth.as_ref() {
        // Auth is configured - verify credentials
        if !verify_basic_auth(auth_header, auth) {
            RPC_ERR_TOTAL.inc();
            return RequestOutcome::error(error_response(id, RpcError::access_denied()), true);
        }
    } else if requires_auth {
        // No auth configured but method requires it - reject with 401
        RPC_ERR_TOTAL.inc();
        tracing::warn!(
            "Protected RPC method '{}' called without authentication configured. \
            Configure rpc_user and rpc_pass to enable wallet operations.",
            method_key
        );
        return RequestOutcome::error(
            error_response(
                id,
                RpcError::access_denied().with_data(
                    "This method requires authentication. Configure rpc_user and rpc_pass."
                ),
            ),
            true,
        );
    }

    match handler.callback()(&server_guard, params.as_slice()) {
        Ok(result) => RequestOutcome::response(success_response(id, result)),
        Err(err) => {
            RPC_ERR_TOTAL.inc();
            RequestOutcome::error(error_response(id, RpcError::from(err)), false)
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
