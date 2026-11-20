use super::middleware::{GovernorRateLimiter, RateLimitConfig};
use super::rpc_error::RpcError;
use super::rpc_server::RpcServer;
use super::rpc_server_settings::RpcServerConfig;

use parking_lot::RwLock;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Weak};
use warp::Filter;
use warp::http::StatusCode;
use warp::http::header::{CONTENT_TYPE, HeaderValue, WWW_AUTHENTICATE};
use warp::reply::Response as HttpResponse;

mod cors;
mod gate;
#[cfg(test)]
#[path = "gate_tests.rs"]
mod gate_tests;
mod handlers;
#[cfg(test)]
mod tests;

pub use cors::BasicAuth;
use cors::{CorsConfig, apply_cors};
pub(crate) use gate::HttpGate;
use handlers::handle_post_request;
#[cfg(feature = "jsonrpsee-server")]
pub(crate) use handlers::{invoke_rpc_handler, resolve_rpc_handler};

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

    let post_route = warp::path::end()
        .and(warp::post())
        .and(with_filters(filters.clone()))
        .and(warp::ext::optional::<SocketAddr>())
        .and(warp::header::optional::<HeaderValue>("origin"))
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::body::content_length_limit(max_post_body_len(
            &settings,
        )))
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
    auth: Arc<Option<BasicAuth>>,
) -> impl Filter<Extract = (HttpResponse,), Error = warp::Rejection> + Clone {
    use warp::reply::Reply;

    let event_tx = std::sync::Arc::new(event_tx);

    warp::path("ws")
        .and(warp::ws())
        .and(warp::header::optional::<String>("authorization"))
        .map(
            move |ws: warp::ws::Ws, authorization: Option<String>| -> HttpResponse {
                // A07: WebSocket upgrades must honor the same BasicAuth gate as HTTP RPC.
                if let Some(credentials) = auth.as_ref()
                    && !cors::verify_basic_auth(authorization.as_deref(), credentials)
                {
                    let mut response = HttpResponse::new(Vec::new().into());
                    *response.status_mut() = StatusCode::UNAUTHORIZED;
                    response.headers_mut().insert(
                        WWW_AUTHENTICATE,
                        HeaderValue::from_static("Basic realm=\"neo-rpc\""),
                    );
                    return response;
                }

                let event_rx = event_tx.subscribe();
                let mgr = subscription_mgr.clone();
                ws.max_frame_size(64 * 1024)
                    .max_message_size(64 * 1024)
                    .on_upgrade(move |socket| super::ws::ws_handler(socket, event_rx, mgr))
                    .into_response()
            },
        )
}

/// Largest POST body accepted, in bytes. The gate receives the body and the
/// POST route checks its declared length; both must use the same limit.
pub(crate) fn max_post_body_len(settings: &RpcServerConfig) -> u64 {
    (settings.max_request_body_size as u64).max(1)
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

/// [`exceeds_max_depth`] for JSON text that has not been parsed yet.
///
/// `enclosing` is the number of arrays and objects the text sits inside, so
/// that a fragment is measured as part of its document. The text is walked
/// once, without recursion and without building a value, which makes this
/// safe to run on input of any depth before a parser sees it.
///
/// The verdict is the one [`exceeds_max_depth`] gives for the parsed
/// document. The only exception is a repeated object key: parsing keeps the
/// last value, this counts them all, so it can only be the stricter of the two.
#[cfg(feature = "jsonrpsee-server")]
fn text_exceeds_max_depth(text: &[u8], enclosing: usize, max_depth: usize) -> bool {
    let mut open = enclosing;
    let mut in_string = false;
    let mut escaped = false;
    for &byte in text {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b' ' | b'\t' | b'\n' | b'\r' | b',' | b':' => {}
            b']' | b'}' => open = open.saturating_sub(1),
            // Anything else belongs to a value (or to a key, which has one)
            // lying one level below the containers open around it.
            _ => {
                if open >= max_depth {
                    return true;
                }
                match byte {
                    b'[' | b'{' => open += 1,
                    b'"' => in_string = true,
                    _ => {}
                }
            }
        }
    }
    false
}

/// Whether a request carrying these `params`, given as unparsed JSON text,
/// is one the routes here refuse for its depth.
///
/// The limit applies to the whole body, so `params` are measured where they
/// lie in it: inside the request object and, for a request sent in a batch,
/// inside the batch array as well.
#[cfg(feature = "jsonrpsee-server")]
pub(crate) fn params_exceed_max_depth(params: &str, batched: bool) -> bool {
    let enclosing = if batched { 2 } else { 1 };
    text_exceeds_max_depth(params.as_bytes(), enclosing, MAX_PARAMS_DEPTH)
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

    pub(super) fn without_response(mut self) -> Self {
        self.response = None;
        self
    }
}
