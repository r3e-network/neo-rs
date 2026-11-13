//! Fail-closed RPC reverse proxies and a separate capability-scoped admin API.

use crate::audit::AuditLog;
use crate::config::TokenDigest;
use crate::executor::BlockExecutor;
use crate::state::{GateRegistry, ProxyPermit};
use crate::upstream::{HttpNode, NodeRpc};
use anyhow::{Context, Result};
use hyper::body::HttpBody;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server, StatusCode};
use neo_primitives::UInt256;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

#[derive(Clone)]
pub struct ProxyGateway {
    node: Arc<HttpNode>,
    gates: GateRegistry,
    allowed_methods: Arc<BTreeSet<String>>,
    proxy_token: Option<TokenDigest>,
    max_request_bytes: usize,
}

impl ProxyGateway {
    pub fn new(
        node: Arc<HttpNode>,
        gates: GateRegistry,
        allowed_methods: Arc<BTreeSet<String>>,
        proxy_token: Option<TokenDigest>,
        max_request_bytes: usize,
    ) -> Self {
        Self {
            node,
            gates,
            allowed_methods,
            proxy_token,
            max_request_bytes,
        }
    }

    pub async fn serve(self, listen: SocketAddr) -> Result<()> {
        let make = make_service_fn(move |_| {
            let gateway = self.clone();
            async move {
                Ok::<_, Infallible>(service_fn(move |request| {
                    let gateway = gateway.clone();
                    async move { Ok::<_, Infallible>(gateway.handle(request).await) }
                }))
            }
        });
        Server::try_bind(&listen)?
            .serve(make)
            .await
            .context("proxy server stopped")
    }

    async fn handle(&self, request: Request<Body>) -> Response<Body> {
        if request.method() != hyper::Method::POST {
            return error_response(
                StatusCode::METHOD_NOT_ALLOWED,
                Value::Null,
                -32600,
                "POST required",
            );
        }
        if !bearer_authorized(request.headers(), self.proxy_token.as_ref()) {
            return error_response(
                StatusCode::UNAUTHORIZED,
                Value::Null,
                -32040,
                "unauthorized",
            );
        }
        let bytes = match read_bounded(request.into_body(), self.max_request_bytes).await {
            Ok(bytes) => bytes,
            Err(error) => {
                return error_response(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    Value::Null,
                    -32600,
                    &error.to_string(),
                );
            }
        };
        let request_json: Value = match serde_json::from_slice(&bytes) {
            Ok(value) => value,
            Err(_) => {
                return error_response(
                    StatusCode::BAD_REQUEST,
                    Value::Null,
                    -32700,
                    "invalid JSON",
                );
            }
        };
        let Some(object) = request_json.as_object() else {
            return error_response(
                StatusCode::BAD_REQUEST,
                Value::Null,
                -32600,
                "single JSON-RPC object required",
            );
        };
        let id = object.get("id").cloned().unwrap_or(Value::Null);
        if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
            || !object.contains_key("id")
            || object.get("method").and_then(Value::as_str).is_none()
            || !matches!(
                object.get("params"),
                None | Some(Value::Array(_) | Value::Object(_))
            )
        {
            return error_response(
                StatusCode::BAD_REQUEST,
                id,
                -32600,
                "invalid JSON-RPC request",
            );
        }
        let method = object
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !self.allowed_methods.contains(method) {
            return error_response(
                StatusCode::FORBIDDEN,
                id,
                -32601,
                "method is not allowed by validator policy",
            );
        }
        let permit = match self.gates.proxy_permit(self.node.kind()) {
            Ok(permit) => permit,
            Err(error) => return unavailable(id, &error.to_string()),
        };
        if let Err(error) = self.pin_upstream(permit).await {
            return unavailable(id, &error.to_string());
        }
        if !self.gates.permit_is_current(self.node.kind(), permit) {
            return unavailable(id, "validation state changed before forwarding");
        }
        let response_bytes = match self.node.send_raw(&bytes).await {
            Ok(bytes) => bytes,
            Err(error) => return unavailable(id, &error.to_string()),
        };
        if let Err(error) = self.pin_upstream(permit).await {
            return unavailable(id, &error.to_string());
        }
        if !self.gates.permit_is_current(self.node.kind(), permit) {
            return unavailable(
                id,
                "validation state changed while the request was in flight",
            );
        }
        match serde_json::from_slice::<Value>(&response_bytes) {
            Ok(response)
                if response.get("jsonrpc").and_then(Value::as_str) == Some("2.0")
                    && response.get("id") == Some(&id) => {}
            _ => return unavailable(id, "upstream returned an invalid JSON-RPC response"),
        }
        Response::builder()
            .status(StatusCode::OK)
            .header(hyper::header::CONTENT_TYPE, "application/json")
            .body(Body::from(response_bytes))
            .unwrap_or_else(|_| unavailable(id, "failed to construct proxy response"))
    }

    async fn pin_upstream(&self, permit: ProxyPermit) -> Result<()> {
        let count = self.node.block_count().await?;
        if count == 0 || count - 1 != permit.verified_height {
            anyhow::bail!(
                "upstream height {} is outside verified watermark {}",
                count.saturating_sub(1),
                permit.verified_height
            );
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct AdminApi {
    gates: GateRegistry,
    executor: Arc<dyn BlockExecutor>,
    audit: AuditLog,
    read_token: TokenDigest,
    admin_token: TokenDigest,
    max_request_bytes: usize,
}

impl AdminApi {
    pub fn new(
        gates: GateRegistry,
        executor: Arc<dyn BlockExecutor>,
        audit: AuditLog,
        read_token: TokenDigest,
        admin_token: TokenDigest,
        max_request_bytes: usize,
    ) -> Self {
        Self {
            gates,
            executor,
            audit,
            read_token,
            admin_token,
            max_request_bytes,
        }
    }

    pub async fn serve(self, listen: SocketAddr) -> Result<()> {
        let make = make_service_fn(move |_| {
            let api = self.clone();
            async move {
                Ok::<_, Infallible>(service_fn(move |request| {
                    let api = api.clone();
                    async move { Ok::<_, Infallible>(api.handle(request).await) }
                }))
            }
        });
        Server::try_bind(&listen)?
            .serve(make)
            .await
            .context("admin server stopped")
    }

    async fn handle(&self, request: Request<Body>) -> Response<Body> {
        let method = request.method().clone();
        let path = request.uri().path().to_owned();
        let is_admin = path == "/resume";
        let expected = if is_admin {
            &self.admin_token
        } else {
            &self.read_token
        };
        let read_authorized =
            path == "/status" && bearer_matches(request.headers(), &self.admin_token);
        if !bearer_authorized(request.headers(), Some(expected)) && !read_authorized {
            return error_response(
                StatusCode::UNAUTHORIZED,
                Value::Null,
                -32040,
                "unauthorized",
            );
        }
        match (method, path.as_str()) {
            (hyper::Method::GET, "/status") => match self.executor.height() {
                Ok(local_height) => json_response(
                    StatusCode::OK,
                    json!({
                        "validator": self.gates.status(),
                        "local_executor_height": local_height,
                    }),
                ),
                Err(error) => unavailable(Value::Null, &error.to_string()),
            },
            (hyper::Method::POST, "/resume") => {
                let bytes = match read_bounded(request.into_body(), self.max_request_bytes).await {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        return error_response(
                            StatusCode::PAYLOAD_TOO_LARGE,
                            Value::Null,
                            -32600,
                            &error.to_string(),
                        );
                    }
                };
                let resume: ResumeRequest = match serde_json::from_slice(&bytes) {
                    Ok(request) => request,
                    Err(_) => {
                        return error_response(
                            StatusCode::BAD_REQUEST,
                            Value::Null,
                            -32600,
                            "invalid recovery request",
                        );
                    }
                };
                if resume.confirmation != "resume-at-verified-baseline" {
                    return error_response(
                        StatusCode::BAD_REQUEST,
                        Value::Null,
                        -32600,
                        "explicit recovery confirmation is required",
                    );
                }
                let Ok(hash) = UInt256::parse(&resume.block_hash) else {
                    return error_response(
                        StatusCode::BAD_REQUEST,
                        Value::Null,
                        -32600,
                        "invalid baseline block hash",
                    );
                };
                let local_height = match self.executor.height() {
                    Ok(height) => height,
                    Err(error) => return unavailable(Value::Null, &error.to_string()),
                };
                match self
                    .gates
                    .resume(&resume.incident_id, resume.height, &hash, local_height)
                {
                    Ok(()) => {
                        let record = json!({
                            "incident_id": resume.incident_id,
                            "height": resume.height,
                            "block_hash": resume.block_hash,
                        });
                        if let Err(error) = self.audit.append("manual_recovery", &record) {
                            return unavailable(
                                Value::Null,
                                &format!("recovery audit failed: {error}"),
                            );
                        }
                        json_response(StatusCode::OK, json!({"resumed":true, "phase":"verifying"}))
                    }
                    Err(error) => error_response(
                        StatusCode::CONFLICT,
                        Value::Null,
                        -32051,
                        &error.to_string(),
                    ),
                }
            }
            _ => error_response(StatusCode::NOT_FOUND, Value::Null, -32601, "not found"),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResumeRequest {
    incident_id: String,
    height: u32,
    block_hash: String,
    confirmation: String,
}

async fn read_bounded(mut body: Body, limit: usize) -> Result<Vec<u8>> {
    if body
        .size_hint()
        .upper()
        .is_some_and(|size| size > limit as u64)
    {
        anyhow::bail!("request body exceeds configured limit");
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = body.data().await {
        let chunk = chunk?;
        if bytes.len().saturating_add(chunk.len()) > limit {
            anyhow::bail!("request body exceeds configured limit");
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn bearer_authorized(headers: &hyper::HeaderMap, expected: Option<&TokenDigest>) -> bool {
    match expected {
        Some(expected) => bearer_matches(headers, expected),
        None => true,
    }
}

fn bearer_matches(headers: &hyper::HeaderMap, expected: &TokenDigest) -> bool {
    headers
        .get(hyper::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|token| expected.verify(token))
}

fn unavailable(id: Value, message: &str) -> Response<Body> {
    error_response(StatusCode::SERVICE_UNAVAILABLE, id, -32050, message)
}

fn error_response(status: StatusCode, id: Value, code: i64, message: &str) -> Response<Body> {
    json_response(
        status,
        json!({
            "jsonrpc":"2.0",
            "id":id,
            "error":{"code":code,"message":message},
        }),
    )
}

fn json_response(status: StatusCode, value: Value) -> Response<Body> {
    Response::builder()
        .status(status)
        .header(hyper::header::CONTENT_TYPE, "application/json")
        .body(Body::from(value.to_string()))
        .unwrap_or_else(|_| {
            Response::new(Body::from("{\"error\":\"response construction failed\"}"))
        })
}

#[cfg(test)]
mod tests;
