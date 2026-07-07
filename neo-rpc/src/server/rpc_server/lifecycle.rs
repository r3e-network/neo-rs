//! RPC transport lifecycle.
//!
//! This module owns jsonrpsee startup, shutdown, and session-purge task wiring
//! for `RpcServer`. Keeping lifecycle glue outside `mod.rs` lets the root
//! module stay focused on the server state, handler registry, wallet/session
//! accessors, and rate-limit policy.

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::{Arc, Weak};
use std::time::Duration;

use parking_lot::RwLock;
use rustls::ServerConfig;
use tokio::{sync::oneshot, time::sleep};
use tracing::{error, info, warn};

use super::RpcServer;
use super::http_policy::RpcHttpLayer;
use crate::server::jsonrpsee_adapter::build_jsonrpsee_module_with_methods;
use crate::server::rpc_transport::log_join_error;

impl RpcServer {
    /// Start the jsonrpsee RPC server.
    pub fn start_rpc_server(
        &mut self,
        handle: Weak<RwLock<Self>>,
        _tls_config: Option<Arc<ServerConfig>>,
    ) {
        if self.started {
            return;
        }

        self.self_handle = Some(handle.clone());

        let auth_credentials = match self.rpc_auth_credentials() {
            Ok(credentials) => credentials,
            Err(err) => {
                error!("invalid RPC authentication configuration: {}", err);
                return;
            }
        };

        // Security warning for production deployments without TLS.
        let has_auth = auth_credentials.is_some();
        let is_localhost = self.settings.bind_address.is_loopback();
        if has_auth && !is_localhost {
            warn!(
                "SECURITY WARNING: RPC server is binding to non-localhost address ({}) with \
                 authentication enabled but WITHOUT TLS encryption. Credentials will be \
                 transmitted in plaintext. For production use, either:\n\
                   1. Place the RPC server behind a TLS-terminating reverse proxy (nginx, caddy)\n\
                   2. Bind only to localhost (127.0.0.1) and use SSH tunneling\n\
                   3. Use a VPN for network-level encryption",
                self.settings.bind_address
            );
        }

        // The current jsonrpsee 0.24 transport does not yet support
        // in-process TLS termination; the recommendation is to put
        // the node behind a TLS-terminating reverse proxy. The
        // `_tls_config` argument is retained for API compatibility with the
        // previous in-process TLS hook.
        let _ = _tls_config;

        let disabled_methods: Arc<HashSet<String>> = Arc::new(
            self.settings
                .disabled_methods
                .iter()
                .map(|name| name.to_ascii_lowercase())
                .collect(),
        );

        let address = SocketAddr::new(self.settings.bind_address, self.settings.port);
        let std_listener = match std::net::TcpListener::bind(address) {
            Ok(listener) => listener,
            Err(err) => {
                error!("error binding RPC server to {}: {}", address, err);
                return;
            }
        };
        let bound_addr = match std_listener.local_addr() {
            Ok(addr) => addr,
            Err(err) => {
                error!("error getting RPC bound address: {}", err);
                return;
            }
        };
        if let Err(err) = std_listener.set_nonblocking(true) {
            error!("error configuring RPC listener: {}", err);
            return;
        }

        // Build the jsonrpsee module from the registered handlers. The public
        // method names are gathered from `self` (only the inner handler-map
        // lock) rather than by re-reading the `Weak<RwLock<Self>>`, because
        // this method runs under the outer write lock
        // (`server.write().start_rpc_server(...)`); re-locking it here would
        // deadlock RPC startup.
        let methods = self.transport_method_names();
        let module = match build_jsonrpsee_module_with_methods(handle, disabled_methods, methods) {
            Ok(m) => m,
            Err(err) => {
                error!("error building jsonrpsee RPC module: {}", err);
                return;
            }
        };

        // Apply the configured DoS limits to the jsonrpsee builder. These are
        // native builder knobs plus a small HTTP policy middleware: the
        // request-body cap, concurrent-connection cap, batch-request cap, WS
        // keep-alive pings, Basic auth, and CORS headers cover the HTTP
        // resource exhaustion and browser-access surfaces. Per-method rate
        // limiting is enforced in the dispatch path, where JSON-RPC method
        // names are available. jsonrpsee 0.24's `build_from_tcp` still does
        // not expose client IPs to that path, so this process-local limiter is
        // a server-side fallback; use an edge proxy for true per-client limits.
        let max_body = u32::try_from(self.settings.max_request_body_size).unwrap_or(u32::MAX);
        let max_conns = u32::try_from(self.settings.max_concurrent_connections).unwrap_or(u32::MAX);
        let batch_cfg = match u32::try_from(self.settings.max_batch_size).unwrap_or(u32::MAX) {
            0 => jsonrpsee::server::BatchRequestConfig::Disabled,
            n => jsonrpsee::server::BatchRequestConfig::Limit(n),
        };

        // Ping at the request-headers interval and drop connections idle beyond
        // keep_alive_timeout (a negative timeout disables idle reaping).
        let mut ping_cfg = jsonrpsee::server::PingConfig::new().ping_interval(
            self.settings
                .request_headers_timeout_duration()
                .max(Duration::from_secs(1)),
        );
        if let Some(keep_alive) = self.settings.keep_alive_timeout_duration() {
            ping_cfg = ping_cfg.inactive_limit(keep_alive);
        }

        let http_middleware = tower::ServiceBuilder::new()
            .layer(RpcHttpLayer::new(&self.settings, auth_credentials.clone()));

        // jsonrpsee's `build_from_tcp` accepts the pre-bound
        // `std::net::TcpListener` and constructs the HTTP+WS server on top of
        // it. This keeps the HTTP+WS lifecycle in the canonical transport
        // builder instead of hand-rolled server glue.
        let server = match jsonrpsee::server::Server::builder()
            .max_request_body_size(max_body)
            .max_connections(max_conns)
            .set_batch_request_config(batch_cfg)
            .enable_ws_ping(ping_cfg)
            .set_http_middleware(http_middleware)
            .build_from_tcp(std_listener)
        {
            Ok(s) => s,
            Err(err) => {
                error!("error building jsonrpsee server: {}", err);
                return;
            }
        };

        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let handle = server.start(module);

        let task = tokio::spawn(async move {
            tokio::select! {
                _ = shutdown_rx => {
                    handle.stop().ok();
                }
                _ = handle.clone().stopped() => {}
            }
        });

        info!("RPC server bound on {}", bound_addr);

        self.shutdown_signal = Some(shutdown_tx);
        self.server_task = Some(task);
        self.started = true;

        // Background purge of expired sessions to avoid leaks when clients drop without cleanup.
        if self.session_enabled() {
            let interval_secs = self.settings.session_expiration_time.max(1) / 2;
            let interval = Duration::from_secs(interval_secs.max(5));
            let (purge_tx, mut purge_rx) = oneshot::channel();
            let server_handle = self.self_handle.clone();
            let purge_task = tokio::spawn(async move {
                loop {
                    tokio::select! {
                        () = sleep(interval) => {
                            let Some(server) = server_handle.as_ref().and_then(Weak::upgrade) else {
                                break;
                            };
                            server.read().purge_expired_sessions();
                        }
                        _ = &mut purge_rx => break,
                    }
                }
            });
            self.session_purge_shutdown = Some(purge_tx);
            self.session_purge_task = Some(purge_task);
        }
        info!(
            "Starting RPC server on {}:{} (network {})",
            self.settings.bind_address, self.settings.port, self.settings.network
        );
    }

    /// Stop the RPC server, purge sessions, and detach the active wallet.
    pub fn stop_rpc_server(&mut self) {
        if !self.started {
            return;
        }

        if let Some(tx) = self.shutdown_signal.take() {
            let _ = tx.send(());
        }

        if let Some(handle) = self.server_task.take() {
            tokio::spawn(async move {
                if let Err(err) = handle.await {
                    log_join_error(err);
                }
            });
        }

        if let Some(tx) = self.session_purge_shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.session_purge_task.take() {
            tokio::spawn(async move {
                let _ = handle.await;
            });
        }

        // Drop any lingering sessions to avoid carrying over state across restarts.
        self.sessions.lock().clear();
        self.set_wallet(None);

        info!("Stopping RPC server for network {}", self.settings.network);
        self.started = false;
    }

    /// Stop the server and clear all registered runtime state.
    pub fn dispose(&mut self) {
        self.stop_rpc_server();
        self.handler_lookup.write().clear();
        self.set_wallet(None);
        self.sessions.lock().clear();
    }
}
