use super::cors::{apply_cors, verify_basic_auth};
use super::{
    build_http_response, error_response, exceeds_max_depth, success_response, RpcFilters,
    RpcQueryParams, RequestOutcome, MAX_PARAMS_DEPTH,
};
use crate::server::rpc_error::RpcError;
use crate::server::rpc_server::{RPC_ERR_TOTAL, RPC_REQ_TOTAL};
use crate::server::rpc_server_settings::{RpcServerSettings, UnhandledExceptionPolicy};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use bytes::Bytes;
use serde_json::{Map, Value};
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::panic::{self, AssertUnwindSafe};
use tracing::error;
use warp::http::header::HeaderValue;
use warp::reply::Response as HttpResponse;

pub(super) async fn handle_post_request(
    filters: RpcFilters,
    remote: Option<SocketAddr>,
    origin: Option<HeaderValue>,
    auth_header: Option<String>,
    body: Bytes,
) -> Result<HttpResponse, Infallible> {
    let client_ip = remote.map(|addr| addr.ip()).unwrap_or_else(|| {
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

pub(super) async fn handle_get_request(
    filters: RpcFilters,
    remote: Option<SocketAddr>,
    origin: Option<HeaderValue>,
    auth_header: Option<String>,
    raw_query: String,
    max_query_len: u64,
) -> Result<HttpResponse, Infallible> {
    let client_ip = remote.map(|addr| addr.ip()).unwrap_or_else(|| {
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

    let method_from_query = query_to_request_value(&raw_query)
        .and_then(|v| v.get("method").and_then(|m| m.as_str()).map(String::from));

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
                let outcome = process_object(obj, &filters, auth_header.as_deref(), Some(client_ip));
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

pub(super) fn process_body(
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

    if entries.len() > filters.max_batch_size {
        return (
            Some(error_response(
                None,
                RpcError::new(
                    -32600,
                    format!(
                        "Batch too large: {} entries exceeds maximum of {}",
                        entries.len(),
                        filters.max_batch_size
                    ),
                    None,
                ),
            )),
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

pub(super) fn query_to_request_value(raw_query: &str) -> Option<Value> {
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

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "panic".to_string()
    }
}
