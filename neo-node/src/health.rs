//! Health endpoint for neo-node.
//!
//! Provides a simple HTTP health check endpoint for monitoring.

pub const DEFAULT_MAX_HEADER_LAG: u32 = 300;

#[derive(Debug, Clone, Default)]
pub struct HealthState {
    pub current_height: u32,
    pub header_height: u32,
    pub block_height: u32,
    pub peer_count: u32,
    pub mempool_size: u32,
    pub is_syncing: bool,
}

use std::future::Future;
use std::sync::Arc;
use tokio::sync::RwLock;
use warp::Filter;

/// Serves the health endpoint with shared runtime state until shutdown resolves.
pub async fn serve_health_with_state<F>(
    port: u16,
    max_header_lag: u32,
    storage_path: Option<String>,
    rpc_enabled: bool,
    health_state: Arc<RwLock<HealthState>>,
    shutdown: F,
) -> anyhow::Result<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    let health_state_filter = warp::any().map(move || health_state.clone());
    let max_lag_filter = warp::any().map(move || max_header_lag);
    let storage_filter = warp::any().map(move || storage_path.clone());
    let rpc_filter = warp::any().map(move || rpc_enabled);

    let health_route = warp::path("health")
        .and(health_state_filter)
        .and(max_lag_filter)
        .and(storage_filter)
        .and(rpc_filter)
        .and_then(|state: Arc<RwLock<HealthState>>, max_lag: u32, storage: Option<String>, rpc: bool| async move {
            let state = state.read().await;
            let lag = state.header_height.saturating_sub(state.current_height);
            let healthy = lag <= max_lag;

            let status = if healthy { warp::http::StatusCode::OK } else { warp::http::StatusCode::SERVICE_UNAVAILABLE };

            let response = serde_json::json!({
                "healthy": healthy,
                "current_height": state.current_height,
                "header_height": state.header_height,
                "lag": lag,
                "max_lag": max_lag,
                "storage": storage,
                "rpc_enabled": rpc,
            });

            Ok::<_, warp::Rejection>(warp::reply::with_status(warp::reply::json(&response), status))
        });

    let (_, server) = warp::serve(health_route)
        .bind_with_graceful_shutdown(([0, 0, 0, 0], port), async move {
            shutdown.await;
        });

    server.await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn health_state_defaults_correctly() {
        let state = HealthState::default();
        assert_eq!(state.current_height, 0);
        assert_eq!(state.header_height, 0);
    }

    #[tokio::test]
    async fn health_server_exits_on_shutdown_signal() {
        let health_state = Arc::new(RwLock::new(HealthState::default()));
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        let server = tokio::spawn(serve_health_with_state(
            0, // Use port 0 for auto-assignment
            DEFAULT_MAX_HEADER_LAG,
            None,
            true,
            health_state,
            async move {
                let _ = shutdown_rx.await;
            },
        ));

        // Give server time to start
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Send shutdown signal
        let _ = shutdown_tx.send(());

        // Server should exit gracefully
        let result = tokio::time::timeout(
            tokio::time::Duration::from_secs(2),
            server
        ).await;

        assert!(result.is_ok(), "Server should exit on shutdown signal");
    }
}
