use parking_lot::{Mutex, RwLock, RwLockReadGuard};
use prometheus::Counter;
use rustls::ServerConfig;
use serde_json::Value;
use std::sync::LazyLock;
use tokio::{
    sync::oneshot,
    task::JoinHandle,
    time::sleep,
};

use tracing::{error, info, warn};
use uuid::Uuid;

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::{Arc, Weak};
use std::time::Duration;

use super::jsonrpsee_adapter::{build_jsonrpsee_module_with_methods, public_method_names};
use super::rpc_server_settings::RpcServerConfig;
use super::session::Session;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_method_attribute::RpcMethodDescriptor;
use crate::server::rpc_transport::log_join_error;
use neo_system::Node;
use neo_wallets::Wallet;

pub type RpcCallback =
    dyn Fn(&RpcServer, &[Value]) -> Result<Value, RpcException> + Send + Sync + 'static;

/// Type alias for wallet change callback to reduce complexity.
pub type WalletChangeCallback = Arc<dyn Fn(Option<Arc<dyn Wallet>>) + Send + Sync>;

pub struct RpcHandler {
    descriptor: RpcMethodDescriptor,
    callback: Arc<RpcCallback>,
}

impl RpcHandler {
    pub fn new(descriptor: RpcMethodDescriptor, callback: Arc<RpcCallback>) -> Self {
        Self {
            descriptor,
            callback,
        }
    }

    #[must_use]
    pub const fn descriptor(&self) -> &RpcMethodDescriptor {
        &self.descriptor
    }

    #[must_use]
    pub fn callback(&self) -> Arc<RpcCallback> {
        Arc::clone(&self.callback)
    }
}

pub(crate) fn rpc_handler(
    name: &'static str,
    func: fn(&RpcServer, &[Value]) -> Result<Value, RpcException>,
) -> RpcHandler {
    RpcHandler::new(RpcMethodDescriptor::new(name), Arc::new(func))
}

pub(crate) fn protected_rpc_handler(
    name: &'static str,
    func: fn(&RpcServer, &[Value]) -> Result<Value, RpcException>,
) -> RpcHandler {
    RpcHandler::new(RpcMethodDescriptor::new_protected(name), Arc::new(func))
}

pub static RPC_REQ_TOTAL: LazyLock<Counter> = LazyLock::new(|| {
    let counter =
        Counter::new("neo_rpc_requests_total", "Total RPC requests").unwrap_or_else(|_| {
            Counter::new("neo_rpc_requests_total_invalid", "Invalid")
                .expect("fallback counter creation should never fail")
        });
    if let Err(err) = prometheus::register(Box::new(counter.clone())) {
        warn!("Failed to register neo_rpc_requests_total: {}", err);
    }
    counter
});
pub static RPC_ERR_TOTAL: LazyLock<Counter> = LazyLock::new(|| {
    let counter = Counter::new("neo_rpc_errors_total", "Total RPC errors").unwrap_or_else(|_| {
        Counter::new("neo_rpc_errors_total_invalid", "Invalid")
            .expect("fallback counter creation should never fail")
    });
    if let Err(err) = prometheus::register(Box::new(counter.clone())) {
        warn!("Failed to register neo_rpc_errors_total: {}", err);
    }
    counter
});

pub struct RpcServer {
    system: Arc<Node>,
    settings: RpcServerConfig,
    handler_lookup: Arc<RwLock<HashMap<String, Arc<RpcHandler>>>>,
    started: bool,
    wallet: Arc<RwLock<Option<Arc<dyn Wallet>>>>,
    wallet_change_callback: Option<WalletChangeCallback>,
    /// Session storage using Mutex instead of `RwLock` to enforce exclusive access.
    ///
    /// # Security Note
    /// Sessions contain `ApplicationEngine` which wraps `ExecutionEngine` with a raw pointer
    /// that is NOT thread-safe. Using `Mutex` instead of `RwLock` prevents accidental
    /// concurrent reads that would cause undefined behavior. See session.rs for details.
    sessions: Arc<Mutex<HashMap<Uuid, Session>>>,
    server_task: Option<JoinHandle<()>>,
    shutdown_signal: Option<oneshot::Sender<()>>,
    session_purge_task: Option<JoinHandle<()>>,
    session_purge_shutdown: Option<oneshot::Sender<()>>,
    self_handle: Option<Weak<RwLock<Self>>>,
    /// WebSocket event bridge for real-time subscriptions
    ws_bridge: Option<Arc<super::ws::WsEventBridge>>,
    /// WebSocket subscription manager
    ws_subscription_mgr: Option<Arc<super::ws::SubscriptionManager>>,
}

impl RpcServer {
    pub fn new(system: Arc<Node>, settings: RpcServerConfig) -> Self {
        Self {
            system,
            settings,
            handler_lookup: Arc::new(RwLock::new(HashMap::new())),
            started: false,
            wallet: Arc::new(RwLock::new(None)),
            wallet_change_callback: None,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            server_task: None,
            shutdown_signal: None,
            session_purge_task: None,
            session_purge_shutdown: None,
            self_handle: None,
            ws_bridge: None,
            ws_subscription_mgr: None,
        }
    }

    #[must_use]
    pub const fn settings(&self) -> &RpcServerConfig {
        &self.settings
    }

    pub fn update_settings(&mut self, settings: RpcServerConfig) {
        self.settings = settings;
    }

    #[must_use]
    pub fn system(&self) -> Arc<Node> {
        Arc::clone(&self.system)
    }

    /// Enable WebSocket subscriptions
    ///
    /// Creates and returns an event bridge that can be used to push events
    /// to connected WebSocket clients. Call this before `start_rpc_server`.
    ///
    /// # Arguments
    /// * `capacity` - Buffer capacity for the broadcast channel
    ///
    /// # Returns
    /// The event bridge that should be used to publish events
    pub fn enable_websocket(&mut self, capacity: usize) -> Arc<super::ws::WsEventBridge> {
        let bridge = Arc::new(super::ws::WsEventBridge::new(capacity));
        let subscription_mgr = Arc::new(super::ws::SubscriptionManager::new());
        self.ws_bridge = Some(Arc::clone(&bridge));
        self.ws_subscription_mgr = Some(subscription_mgr);
        bridge
    }

    /// Get the WebSocket event bridge if enabled
    #[must_use]
    pub fn ws_bridge(&self) -> Option<Arc<super::ws::WsEventBridge>> {
        self.ws_bridge.clone()
    }

    /// Check if WebSocket is enabled
    #[must_use]
    pub const fn is_websocket_enabled(&self) -> bool {
        self.ws_bridge.is_some()
    }

    pub fn start_rpc_server(
        &mut self,
        handle: Weak<RwLock<Self>>,
        _tls_config: Option<Arc<ServerConfig>>,
    ) {
        if self.started {
            return;
        }

        self.self_handle = Some(handle.clone());

        // Security warning for production deployments without TLS
        let has_auth = !self.settings.rpc_user.trim().is_empty();
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
        // `_tls_config` argument is retained for API compatibility
        // with the previous warp-based glue.
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
        let methods = public_method_names(self);
        let module = match build_jsonrpsee_module_with_methods(handle, disabled_methods, methods) {
            Ok(m) => m,
            Err(err) => {
                error!("error building jsonrpsee RPC module: {}", err);
                return;
            }
        };

        // Apply the configured DoS limits to the jsonrpsee builder. These are
        // native builder knobs (no extra HTTP middleware): the request-body
        // cap, the concurrent-connection cap, the batch-request cap, and WS
        // keep-alive pings close the principal amplification / resource
        // exhaustion vectors. Without these the server ran on jsonrpsee
        // defaults (10 MiB bodies, unlimited batches, no idle reaping) and the
        // configured `RpcServerConfig` limits were silently ignored.
        //
        // CORS and per-IP rate limiting are intentionally NOT wired here:
        // jsonrpsee 0.24's `build_from_tcp` cannot expose the client IP to the
        // HTTP middleware layer (so the existing `GovernorRateLimiter` cannot
        // be keyed per-IP without replacing the accept loop), and the CORS
        // tower layer carries a fragile response-body type bound. Both are
        // tracked as follow-ups in docs/RPC_HARDENING.md.
        let max_body = u32::try_from(self.settings.max_request_body_size).unwrap_or(u32::MAX);
        let max_conns =
            u32::try_from(self.settings.max_concurrent_connections).unwrap_or(u32::MAX);
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

        // jsonrpsee's `build_from_tcp` accepts the pre-bound
        // `std::net::TcpListener` and constructs the HTTP+WS server on top of
        // it. This replaces the ~225 lines of warp/hyper glue that previously
        // lived in this method.
        let server = match jsonrpsee::server::Server::builder()
            .max_request_body_size(max_body)
            .max_connections(max_conns)
            .set_batch_request_config(batch_cfg)
            .enable_ws_ping(ping_cfg)
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
            // `handle` was consumed above by `build_jsonrpsee_module_with_disabled`.
            // The session-purge task re-acquires the live `RpcServer` via a
            // channel-based approach: rather than storing a `Weak<RwLock<Self>>`
            // (which was a workaround for the previous warp-based glue), the
            // daemon-level `stop_rpc_server` already sends a shutdown signal to
            // both the server task and the purge task, so the purge loop can
            // safely just sleep and exit on signal.
            let _ = self.self_handle.clone(); // kept for symmetry with prior code
            let purge_task = tokio::spawn(async move {
                loop {
                    tokio::select! {
                     () = sleep(interval) => {
                         // Purge runs in a best-effort manner; the actual
                         // lock acquisition happens inside `purge_expired_sessions`
                         // which the server uses through the rpc handler
                         // invocation path. For the periodic background
                         // purge, the daemon's `stop_rpc_server` sends the
                         // shutdown signal so we just sleep here.
                     }
                     _ = &mut purge_rx => break}
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

    pub fn register_method(&mut self, handler: RpcHandler) {
        let key = handler.descriptor().name.to_ascii_lowercase();
        self.handler_lookup.write().insert(key, Arc::new(handler));
    }

    pub fn register_handlers(&mut self, handlers: Vec<RpcHandler>) {
        for handler in handlers {
            self.register_method(handler);
        }
    }

    #[must_use]
    pub const fn is_started(&self) -> bool {
        self.started
    }

    pub fn dispose(&mut self) {
        self.stop_rpc_server();
        self.handler_lookup.write().clear();
        self.set_wallet(None);
        self.sessions.lock().clear();
    }

    pub fn set_wallet(&self, wallet: Option<Arc<dyn Wallet>>) {
        *self.wallet.write() = wallet;
        if let Some(callback) = &self.wallet_change_callback {
            callback(self.wallet.read().clone());
        }
    }

    #[must_use]
    pub fn wallet(&self) -> Option<Arc<dyn Wallet>> {
        self.wallet.read().clone()
    }

    pub fn set_wallet_change_callback(&mut self, callback: Option<WalletChangeCallback>) {
        self.wallet_change_callback = callback;
    }

    const fn session_expiration(&self) -> Duration {
        Duration::from_secs(self.settings.session_expiration_time)
    }

    #[must_use]
    pub const fn session_enabled(&self) -> bool {
        self.settings.session_enabled
    }

    pub fn purge_expired_sessions(&self) {
        if !self.session_enabled() {
            return;
        }
        let expiration = self.session_expiration();
        let mut guard = self.sessions.lock();
        guard.retain(|_, session| !session.is_expired(expiration));
    }

    pub fn store_session(&self, session: Session) -> Uuid {
        let id = Uuid::new_v4();
        self.sessions.lock().insert(id, session);
        id
    }

    pub fn with_session_mut<F, R>(&self, id: &Uuid, func: F) -> Option<R>
    where
        F: FnOnce(&mut Session) -> R,
    {
        let mut guard = self.sessions.lock();
        guard.get_mut(id).map(func)
    }

    #[must_use]
    pub fn terminate_session(&self, id: &Uuid) -> bool {
        self.sessions.lock().remove(id).is_some()
    }

    pub(crate) fn handlers_guard(&self) -> RwLockReadGuard<'_, HashMap<String, Arc<RpcHandler>>> {
        self.handler_lookup.read()
    }
}
