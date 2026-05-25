//! Health endpoint for neo-node.
//!
//! The HTTP implementation lives in `neo-telemetry`; neo-node only binds the
//! node storage-version policy to that shared endpoint.

pub use neo_telemetry::{HealthState, DEFAULT_MAX_HEADER_LAG};

use std::future::Future;
use std::sync::Arc;
use tokio::sync::RwLock;

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
    neo_telemetry::serve_health_with_state(
        port,
        max_header_lag,
        storage_path,
        crate::startup::STORAGE_VERSION.to_string(),
        rpc_enabled,
        health_state,
        shutdown,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper::{Client, StatusCode, Uri};
    use std::net::TcpListener;
    use tokio::sync::oneshot;
    use tokio::time::{sleep, timeout, Duration};

    #[tokio::test]
    async fn health_server_exits_on_shutdown_signal() {
        let port = free_local_port();
        let health_state = Arc::new(RwLock::new(HealthState::default()));
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        let server = tokio::spawn(serve_health_with_state(
            port,
            DEFAULT_MAX_HEADER_LAG,
            None,
            true,
            health_state,
            async move {
                let _ = shutdown_rx.await;
            },
        ));

        let uri = format!("http://127.0.0.1:{port}/healthz")
            .parse::<Uri>()
            .expect("valid health URI");
        wait_for_ok_health(uri).await;

        let _ = shutdown_tx.send(());
        timeout(Duration::from_secs(1), server)
            .await
            .expect("health server should stop")
            .expect("health server task should join")
            .expect("health server shutdown should be clean");
    }

    fn free_local_port() -> u16 {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind local port");
        listener.local_addr().expect("local address").port()
    }

    async fn wait_for_ok_health(uri: Uri) {
        let client = Client::new();
        for _ in 0..50 {
            match client.get(uri.clone()).await {
                Ok(response) => {
                    assert_eq!(response.status(), StatusCode::OK);
                    return;
                }
                Err(_) => sleep(Duration::from_millis(20)).await,
            }
        }
        panic!("health endpoint did not become available");
    }
}
