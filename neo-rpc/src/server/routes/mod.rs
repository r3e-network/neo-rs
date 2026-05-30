use super::middleware::{GovernorRateLimiter, RateLimitConfig};
use super::rpc_error::RpcError;
use super::rpc_server::RpcServer;
use super::rpc_server_settings::RpcServerConfig;

use parking_lot::RwLock;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Weak};
use warp::http::header::{HeaderValue, CONTENT_TYPE, WWW_AUTHENTICATE};
use warp::http::StatusCode;
use warp::reply::Response as HttpResponse;
use warp::Filter;

mod cors;
mod handlers;
#[cfg(test)]
#[path = "tests.rs"]
mod tests;

pub use cors::BasicAuth;
use cors::{apply_cors, CorsConfig};
use handlers::handle_post_request;
#[cfg(feature = "jsonrpsee-server")]
pub(in crate::server) use handlers::{invoke_rpc_handler, resolve_rpc_handler};

const MAX_PARAMS_DEPTH: usize = 32;

#[derive(Clone)]
struct RpcFilters {
    server: Weak<RwLock<RpcServer>>,
    disabled: Arc<HashSet<String>>,
    auth: Arc<Option<BasicAuth>>,
    rate_limiter: Option<Arc<GovernorRateLimiter>>,
    cors: Option<CorsConfig>,
    max_batch_size: usize,
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
    let max_batch = settings.max_batch_size;
    let filters = RpcFilters {
        server: handle,
        disabled,
        auth,
        rate_limiter,
        cors: CorsConfig::from_settings(&settings, has_auth),
        max_batch_size: max_batch,
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
            handlers::handle_get_request(filters, remote, origin, auth, raw_query, max_query)
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

fn success_response(id: Option<Value>, result: Value) -> Value {
    let mut response = Map::new();
    response.insert("jsonrpc".to_string(), Value::String("2.0".to_string()));
    response.insert("result".to_string(), result);
    response.insert("id".to_string(), id.unwrap_or(Value::Null));
    Value::Object(response)
}

pub(super) fn error_response(id: Option<Value>, error: RpcError) -> Value {
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

pub(super) fn build_http_response(
    body: Option<Value>,
    unauthorized: bool,
    challenge: bool,
) -> HttpResponse {
    let (mut response, _has_body) = if let Some(body) = body {
        // Match the C# RPC server, which serializes every response via
        // `JToken.ToString()` -> `Utf8JsonWriter` with `JavaScriptEncoder.Default`:
        // `<` `>` `&` `'` `+` `` ` `` and all non-ASCII code points are emitted as
        // `\uXXXX`. serde_json's default serializer only escapes the JSON-mandatory
        // set, so route through the C#-compatible escaping formatter instead.
        let json = neo_json::escape::to_vec(&body, false).unwrap_or_else(|_| b"{}".to_vec());
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

pub(super) struct RequestOutcome {
    pub(super) response: Option<Value>,
    pub(super) unauthorized: bool,
}

pub(super) fn exceeds_max_depth(value: &Value, max_depth: usize) -> bool {
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
    pub(super) const fn response(value: Value) -> Self {
        Self {
            response: Some(value),
            unauthorized: false,
        }
    }

    pub(super) const fn error(value: Value, unauthorized: bool) -> Self {
        Self {
            response: Some(value),
            unauthorized,
        }
    }

    pub(super) const fn notification() -> Self {
        Self {
            response: None,
            unauthorized: false,
        }
    }
}
