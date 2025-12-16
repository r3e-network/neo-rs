//! Health endpoint for neo-node.
//!
//! Provides health and readiness checks for the node runtime,
//! including block height, peer count, mempool size, and storage status.

pub const DEFAULT_MAX_HEADER_LAG: u32 = 20;

use crate::metrics;
use hyper::{
    service::{make_service_fn, service_fn},
    Body, Request, Response, Server, StatusCode,
};
use serde::Serialize;
use std::fs;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Shared health state updated by the node runtime
#[derive(Debug, Clone, Default)]
pub struct HealthState {
    pub block_height: u32,
    pub header_height: u32,
    pub peer_count: usize,
    pub mempool_size: u32,
    pub is_syncing: bool,
}

/// Serves the health endpoint with real-time node state.
///
/// Provides health checks including block height, peer count, mempool size,
/// and storage status for monitoring and orchestration systems.
pub async fn serve_health(
    port: u16,
    max_header_lag: u32,
    storage_path: Option<String>,
    rpc_enabled: bool,
) -> anyhow::Result<()> {
    serve_health_with_state(
        port,
        max_header_lag,
        storage_path,
        rpc_enabled,
        Arc::new(RwLock::new(HealthState::default())),
    )
    .await
}

/// Serves the health endpoint with shared state from the runtime.
pub async fn serve_health_with_state(
    port: u16,
    max_header_lag: u32,
    storage_path: Option<String>,
    rpc_enabled: bool,
    health_state: Arc<RwLock<HealthState>>,
) -> anyhow::Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    let make_svc = make_service_fn(move |_conn| {
        let storage_path = storage_path.clone();
        let health_state = health_state.clone();
        async move {
            let storage_path_inner = storage_path.clone();
            let health_state_inner = health_state.clone();
            Ok::<_, hyper::Error>(service_fn(move |req| {
                let storage_path_req = storage_path_inner.clone();
                let health_state_req = health_state_inner.clone();
                async move {
                    handle_request(req, storage_path_req, rpc_enabled, max_header_lag, health_state_req).await
                }
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
    max_header_lag: u32,
    health_state: Arc<RwLock<HealthState>>,
) -> Result<Response<Body>, hyper::Error> {
    match (req.method(), req.uri().path()) {
        (&hyper::Method::GET, "/healthz") | (&hyper::Method::GET, "/readyz") => {
            let storage_ready = storage_path
                .as_deref()
                .map(|path| verify_storage_markers(path, 0))
                .unwrap_or(true);

            let state = health_state.read().await;

            // Check if node is healthy based on header lag
            let header_lag = state.header_height.saturating_sub(state.block_height);
            let is_synced = header_lag <= max_header_lag;
            let is_healthy = storage_ready && (is_synced || state.is_syncing);

            let body = HealthStatus {
                status: if is_healthy { "ok" } else { "degraded" },
                version: env!("CARGO_PKG_VERSION"),
                rpc_enabled,
                storage_ready,
                block_height: state.block_height,
                header_height: state.header_height,
                peer_count: state.peer_count,
                mempool_size: state.mempool_size,
                is_syncing: state.is_syncing,
                header_lag,
            };

            let json = serde_json::to_string(&body).unwrap_or_else(|_| r#"{"status":"ok"}"#.into());
            let mut resp = Response::new(Body::from(json));
            if !is_healthy {
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
    rpc_enabled: bool,
    storage_ready: bool,
    block_height: u32,
    header_height: u32,
    peer_count: usize,
    mempool_size: u32,
    is_syncing: bool,
    header_lag: u32,
}

fn verify_storage_markers(path: &str, _expected_magic: u32) -> bool {
    let storage_path = std::path::Path::new(path);
    let version_marker = storage_path.join("VERSION");

    // Verify storage version marker exists and matches expected version
    fs::read_to_string(&version_marker)
        .ok()
        .map(|contents| contents.trim() == crate::STORAGE_VERSION)
        .unwrap_or(true) // Allow missing marker for new installations
}
