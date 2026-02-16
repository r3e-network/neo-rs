//! Node health endpoint
//!
//! Provides HTTP health and readiness checks for the node runtime,
//! including block height, peer count, mempool size, and storage status.

use hyper::{
    Body, Request, Response, Server, StatusCode,
    service::{make_service_fn, service_fn},
};
use serde::Serialize;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Default maximum acceptable header lag
pub const DEFAULT_MAX_HEADER_LAG: u32 = 20;

/// Shared health state updated by the node runtime
#[derive(Debug, Clone, Default)]
pub struct HealthState {
    /// Current block height
    pub block_height: u32,
    /// Current header height
    pub header_height: u32,
    /// Number of connected peers
    pub peer_count: usize,
    /// Mempool transaction count
    pub mempool_size: u32,
    /// Is node currently syncing
    pub is_syncing: bool,
}

impl HealthState {
    /// Calculate header lag
    pub fn header_lag(&self) -> u32 {
        self.header_height.saturating_sub(self.block_height)
    }

    /// Check if node is synced (within acceptable lag)
    pub fn is_synced(&self, max_lag: u32) -> bool {
        self.header_lag() <= max_lag
    }

    /// Overall health check
    pub fn is_healthy(&self, max_lag: u32) -> bool {
        self.is_synced(max_lag) || self.is_syncing
    }
}

/// Health check HTTP server for neo-node
pub struct NodeHealthServer {
    state: Arc<RwLock<HealthState>>,
    port: u16,
    max_header_lag: u32,
    storage_path: Option<String>,
    storage_version: String,
    rpc_enabled: bool,
}

impl NodeHealthServer {
    /// Create a new health server
    pub fn new(
        port: u16,
        max_header_lag: u32,
        storage_path: Option<String>,
        storage_version: String,
        rpc_enabled: bool,
    ) -> Self {
        Self {
            state: Arc::new(RwLock::new(HealthState::default())),
            port,
            max_header_lag,
            storage_path,
            storage_version,
            rpc_enabled,
        }
    }

    /// Get a clone of the state handle for updates
    pub fn state_handle(&self) -> Arc<RwLock<HealthState>> {
        self.state.clone()
    }

    /// Update health state
    pub async fn update_state<F>(&self, f: F)
    where
        F: FnOnce(&mut HealthState),
    {
        let mut state = self.state.write().await;
        f(&mut state);
    }

    /// Start the health server
    pub async fn start(self) -> anyhow::Result<()> {
        let addr = SocketAddr::from(([127, 0, 0, 1], self.port));

        let state = self.state.clone();
        let storage_path = self.storage_path.clone();
        let storage_version = self.storage_version.clone();
        let rpc_enabled = self.rpc_enabled;
        let max_header_lag = self.max_header_lag;

        let make_svc = make_service_fn(move |_conn| {
            let state = state.clone();
            let storage_path = storage_path.clone();
            let storage_version = storage_version.clone();

            async move {
                Ok::<_, hyper::Error>(service_fn(move |req| {
                    handle_request(
                        req,
                        state.clone(),
                        storage_path.clone(),
                        storage_version.clone(),
                        rpc_enabled,
                        max_header_lag,
                    )
                }))
            }
        });

        tracing::info!("Health server starting on {}", addr);
        Server::bind(&addr).serve(make_svc).await?;
        Ok(())
    }
}

async fn handle_request(
    req: Request<Body>,
    state: Arc<RwLock<HealthState>>,
    storage_path: Option<String>,
    storage_version: String,
    rpc_enabled: bool,
    max_header_lag: u32,
) -> Result<Response<Body>, hyper::Error> {
    match (req.method(), req.uri().path()) {
        (&hyper::Method::GET, "/healthz") | (&hyper::Method::GET, "/readyz") => {
            let storage_ready = storage_path
                .as_deref()
                .map(|path| verify_storage_markers(path, &storage_version))
                .unwrap_or(true);

            let state = state.read().await;
            let header_lag = state.header_lag();
            let is_synced = state.is_synced(max_header_lag);
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
            let body = crate::node_metrics::gather_prometheus();
            Ok(Response::new(Body::from(body)))
        }
        (&hyper::Method::GET, "/") => {
            let info = serde_json::json!({
                "service": "neo-node",
                "version": env!("CARGO_PKG_VERSION"),
                "endpoints": ["/healthz", "/readyz", "/metrics"]
            });
            Ok(Response::new(Body::from(info.to_string())))
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

fn verify_storage_markers(path: &str, expected_version: &str) -> bool {
    let storage_path = std::path::Path::new(path);
    let version_marker = storage_path.join("VERSION");

    std::fs::read_to_string(&version_marker)
        .ok()
        .map(|contents| contents.trim() == expected_version)
        .unwrap_or(true) // Allow missing marker for new installations
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_state_default() {
        let state = HealthState::default();
        assert_eq!(state.block_height, 0);
        assert_eq!(state.header_lag(), 0);
        assert!(state.is_synced(DEFAULT_MAX_HEADER_LAG));
    }

    #[test]
    fn test_health_state_lag() {
        let state = HealthState {
            block_height: 100,
            header_height: 125,
            ..Default::default()
        };
        assert_eq!(state.header_lag(), 25);
        assert!(!state.is_synced(DEFAULT_MAX_HEADER_LAG));
        assert!(state.is_synced(30));
    }

    #[test]
    fn test_storage_version_check() {
        // New installation (no marker) should pass
        assert!(verify_storage_markers("/nonexistent", "1.0"));
    }
}
