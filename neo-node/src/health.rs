//! Minimal health endpoint for neo-node.
//!
//! NOTE: Health endpoint is temporarily simplified during Phase 2 refactoring.
//! Full health checks will be restored when the node runtime is reimplemented
//! using the new modular architecture (neo-state, neo-p2p, neo-consensus).

pub const DEFAULT_MAX_HEADER_LAG: u32 = 20;

use crate::metrics;
use hyper::{
    service::{make_service_fn, service_fn},
    Body, Request, Response, Server, StatusCode,
};
use serde::Serialize;
use std::fs;
use std::net::SocketAddr;

/// Serves a simplified health endpoint during refactoring.
///
/// Full health checks (block height, peer count, mempool size, etc.) will be
/// restored when the node runtime is reimplemented.
pub async fn serve_health(
    port: u16,
    _max_header_lag: u32,
    storage_path: Option<String>,
    rpc_enabled: bool,
) -> anyhow::Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    let make_svc = make_service_fn(move |_conn| {
        let storage_path = storage_path.clone();
        async move {
            let storage_path_inner = storage_path.clone();
            Ok::<_, hyper::Error>(service_fn(move |req| {
                let storage_path_req = storage_path_inner.clone();
                async move { handle_request(req, storage_path_req, rpc_enabled).await }
            }))
        }
    });

    Server::bind(&addr).serve(make_svc).await?;
    Ok(())
}

async fn handle_request(
    req: Request<Body>,
    storage_path: Option<String>,
    rpc_enabled: bool,
) -> Result<Response<Body>, hyper::Error> {
    match (req.method(), req.uri().path()) {
        (&hyper::Method::GET, "/healthz") | (&hyper::Method::GET, "/readyz") => {
            // Simplified health check during refactoring
            let storage_ready = storage_path
                .as_deref()
                .map(|path| verify_storage_markers(path, 0)) // Magic check disabled
                .unwrap_or(true);

            let body = HealthStatus {
                status: if storage_ready { "ok" } else { "degraded" },
                version: env!("CARGO_PKG_VERSION"),
                refactoring_note: "Node runtime is being refactored. Full health checks pending.",
                rpc_enabled,
                storage_ready,
                // Placeholder values during refactoring
                block_height: 0,
                header_height: 0,
                peer_count: 0,
                mempool_size: 0,
            };

            let json =
                serde_json::to_string(&body).unwrap_or_else(|_| r#"{"status":"ok"}"#.into());
            let mut resp = Response::new(Body::from(json));
            if !storage_ready {
                *resp.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
            }
            Ok(resp)
        }
        (&hyper::Method::GET, "/metrics") => {
            let body = metrics::gather();
            Ok(Response::new(Body::from(body)))
        }
        _ => {
            let mut not_found = Response::new(Body::from("not found"));
            *not_found.status_mut() = StatusCode::NOT_FOUND;
            Ok(not_found)
        }
    }
}

#[derive(Serialize)]
struct HealthStatus {
    status: &'static str,
    version: &'static str,
    refactoring_note: &'static str,
    rpc_enabled: bool,
    storage_ready: bool,
    block_height: u32,
    header_height: u32,
    peer_count: usize,
    mempool_size: u32,
}

fn verify_storage_markers(path: &str, _expected_magic: u32) -> bool {
    let storage_path = std::path::Path::new(path);
    let version_marker = storage_path.join("VERSION");

    // Only check version marker during refactoring
    fs::read_to_string(&version_marker)
        .ok()
        .map(|contents| contents.trim() == crate::STORAGE_VERSION)
        .unwrap_or(true) // Allow missing marker during refactoring
}
