//! HTTP routing for local telemetry endpoints.

use std::convert::Infallible;
use std::sync::Arc;

use hyper::header::CONTENT_TYPE;
use hyper::{Body, Request, Response, StatusCode};
use prometheus::{Encoder, TextEncoder};
use serde_json::json;
use tracing::warn;

use super::super::config::{TELEMETRY_HEALTH_PATH, TELEMETRY_READY_PATH};
use super::exporter::MetricsExporter;

pub(super) async fn serve_metrics_request(
    request: Request<Body>,
    path: String,
    exporter: Arc<MetricsExporter>,
) -> Result<Response<Body>, Infallible> {
    let request_path = request.uri().path();
    if request_path != path
        && request_path != TELEMETRY_HEALTH_PATH
        && request_path != TELEMETRY_READY_PATH
    {
        return Ok(response_with_status(StatusCode::NOT_FOUND, "not found"));
    }
    if request.method() != hyper::Method::GET {
        return Ok(response_with_status(
            StatusCode::METHOD_NOT_ALLOWED,
            "method not allowed",
        ));
    }

    if request_path == TELEMETRY_HEALTH_PATH {
        return Ok(json_response(
            StatusCode::OK,
            json!({
                "status": "ok",
                "service": "neo-node",
                "version": env!("CARGO_PKG_VERSION"),
            }),
        ));
    }

    if request_path == TELEMETRY_READY_PATH {
        return Ok(exporter.readiness_response());
    }

    match exporter.render() {
        Ok(body) => {
            let response = Response::builder()
                .status(StatusCode::OK)
                .header(CONTENT_TYPE, TextEncoder::new().format_type())
                .body(Body::from(body))
                .unwrap_or_else(|_| response_with_status(StatusCode::INTERNAL_SERVER_ERROR, ""));
            Ok(response)
        }
        Err(err) => {
            warn!(
                target: "neo::telemetry",
                error = %err,
                "failed to render metrics"
            );
            Ok(response_with_status(
                StatusCode::INTERNAL_SERVER_ERROR,
                "metrics render failed",
            ))
        }
    }
}

pub(super) fn response_with_status(status: StatusCode, body: &'static str) -> Response<Body> {
    Response::builder()
        .status(status)
        .body(Body::from(body))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

pub(super) fn json_response(status: StatusCode, body: serde_json::Value) -> Response<Body> {
    let bytes = serde_json::to_vec(&body).unwrap_or_else(|_| b"{}".to_vec());
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}
