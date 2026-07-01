//! # neo-rpc::server::rpc_server
//!
//! Core RPC server trait and callback registry.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `http_policy`: HTTP policy helpers for the RPC server.

mod http_policy;

use parking_lot::{Mutex, RwLock, RwLockReadGuard};
use prometheus::Counter;
use rustls::ServerConfig;
use serde_json::Value;
use std::sync::LazyLock;
use tokio::{sync::oneshot, task::JoinHandle, time::sleep};

use tracing::{error, info, warn};
use uuid::Uuid;

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Weak};
use std::time::Duration;

use self::http_policy::RpcHttpLayer;
use super::jsonrpsee_adapter::build_jsonrpsee_module_with_methods;
use super::middleware::{GovernorRateLimiter, RateLimitCheckResult, RateLimitConfig};
use super::rpc_error::RpcError;
use super::rpc_remote_ledger::RemoteLedgerRpcClient;
use super::rpc_server_settings::RpcServerConfig;
use super::session::Session;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_method_attribute::RpcMethodDescriptor;
use crate::server::rpc_transport::log_join_error;
use neo_system::Node;
use neo_wallets::Wallet;

/// Callback signature used by registered RPC handlers.
pub type RpcCallback =
    dyn Fn(&RpcServer, &[Value]) -> Result<Value, RpcException> + Send + Sync + 'static;

/// Type alias for wallet change callback to reduce complexity.
pub type WalletChangeCallback = Arc<dyn Fn(Option<Arc<dyn Wallet>>) + Send + Sync>;

/// Registered RPC method descriptor and callback.
pub struct RpcHandler {
    descriptor: RpcMethodDescriptor,
    callback: Arc<RpcCallback>,
}

impl RpcHandler {
    /// Create an RPC handler from a method descriptor and callback.
    pub fn new(descriptor: RpcMethodDescriptor, callback: Arc<RpcCallback>) -> Self {
        Self {
            descriptor,
            callback,
        }
    }

    /// Return this handler's method descriptor.
    #[must_use]
    pub const fn descriptor(&self) -> &RpcMethodDescriptor {
        &self.descriptor
    }

    /// Return a clone of this handler's callback.
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

/// Total number of RPC requests dispatched by this process.
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

/// Total number of RPC requests that returned an RPC error.
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

/// JSON-RPC server for a Neo node.
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
    /// Process-wide fallback limiter for RPC transports that do not expose client IPs.
    rate_limiter: Arc<GovernorRateLimiter>,
    remote_ledger_rpc: Option<RemoteLedgerRpcClient>,
}

impl RpcServer {
    /// Create an RPC server with the given node system and server settings.
    pub fn new(system: Arc<Node>, settings: RpcServerConfig) -> Self {
        let rate_limiter = Arc::new(rate_limiter_from_settings(&settings));
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
            rate_limiter,
            remote_ledger_rpc: None,
        }
    }

    /// Return this server's current settings.
    #[must_use]
    pub const fn settings(&self) -> &RpcServerConfig {
        &self.settings
    }

    /// Replace this server's settings.
    pub fn update_settings(&mut self, settings: RpcServerConfig) {
        self.rate_limiter = Arc::new(rate_limiter_from_settings(&settings));
        self.settings = settings;
    }

    /// Return the node system served by this RPC instance.
    #[must_use]
    pub fn system(&self) -> Arc<Node> {
        Arc::clone(&self.system)
    }

    /// Configure an upstream RPC endpoint for read-only ledger queries.
    pub fn set_remote_ledger_rpc(&mut self, endpoint: impl Into<String>) -> Result<(), RpcError> {
        self.remote_ledger_rpc = Some(RemoteLedgerRpcClient::new(endpoint)?);
        Ok(())
    }

    /// Return the configured upstream ledger RPC client, if any.
    #[must_use]
    pub fn remote_ledger_rpc(&self) -> Option<&RemoteLedgerRpcClient> {
        self.remote_ledger_rpc.as_ref()
    }

    /// Apply configured server-side rate limiting for one RPC method call.
    pub(crate) fn check_rate_limit(&self, method: &str) -> Result<(), RpcError> {
        match self
            .rate_limiter
            .check_for_method(global_rate_limit_key(), method)
        {
            RateLimitCheckResult::Allowed | RateLimitCheckResult::Disabled => Ok(()),
            RateLimitCheckResult::Blocked => Err(RpcError::too_many_requests()),
        }
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
        self.ws_bridge = Some(Arc::clone(&bridge));
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
            // `handle` was consumed above by `build_jsonrpsee_module_with_disabled`.
            // The session-purge task re-acquires the live `RpcServer` via a
            // channel-based approach: the daemon-level `stop_rpc_server`
            // already sends a shutdown signal to both the server task and the
            // purge task, so the purge loop can safely just sleep and exit on
            // signal.
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

    /// Register a single RPC handler.
    pub fn register_method(&mut self, handler: RpcHandler) {
        let key = handler.descriptor().name.to_ascii_lowercase();
        self.handler_lookup.write().insert(key, Arc::new(handler));
    }

    /// Register multiple RPC handlers.
    pub fn register_handlers(&mut self, handlers: Vec<RpcHandler>) {
        for handler in handlers {
            self.register_method(handler);
        }
    }

    /// Return whether the RPC server has been started.
    #[must_use]
    pub const fn is_started(&self) -> bool {
        self.started
    }

    /// Stop the server and clear all registered runtime state.
    pub fn dispose(&mut self) {
        self.stop_rpc_server();
        self.handler_lookup.write().clear();
        self.set_wallet(None);
        self.sessions.lock().clear();
    }

    /// Set or clear the wallet exposed to wallet RPC methods.
    pub fn set_wallet(&self, wallet: Option<Arc<dyn Wallet>>) {
        *self.wallet.write() = wallet;
        if let Some(callback) = &self.wallet_change_callback {
            callback(self.wallet.read().clone());
        }
    }

    /// Return the wallet currently exposed to wallet RPC methods.
    #[must_use]
    pub fn wallet(&self) -> Option<Arc<dyn Wallet>> {
        self.wallet.read().clone()
    }

    /// Install a callback invoked whenever the active wallet changes.
    pub fn set_wallet_change_callback(&mut self, callback: Option<WalletChangeCallback>) {
        self.wallet_change_callback = callback;
    }

    const fn session_expiration(&self) -> Duration {
        Duration::from_secs(self.settings.session_expiration_time)
    }

    /// Return whether invoke sessions are enabled.
    #[must_use]
    pub const fn session_enabled(&self) -> bool {
        self.settings.session_enabled
    }

    /// Remove expired RPC invoke sessions.
    pub fn purge_expired_sessions(&self) {
        if !self.session_enabled() {
            return;
        }
        let expiration = self.session_expiration();
        let mut guard = self.sessions.lock();
        guard.retain(|_, session| !session.is_expired(expiration));
    }

    /// Store an invoke session and return its generated id.
    pub fn store_session(&self, session: Session) -> Uuid {
        let id = Uuid::new_v4();
        self.sessions.lock().insert(id, session);
        id
    }

    /// Mutably access a stored session by id.
    pub fn with_session_mut<F, R>(&self, id: &Uuid, func: F) -> Option<R>
    where
        F: FnOnce(&mut Session) -> R,
    {
        let mut guard = self.sessions.lock();
        guard.get_mut(id).map(func)
    }

    /// Remove a stored session by id.
    #[must_use]
    pub fn terminate_session(&self, id: &Uuid) -> bool {
        self.sessions.lock().remove(id).is_some()
    }

    pub(crate) fn handlers_guard(&self) -> RwLockReadGuard<'_, HashMap<String, Arc<RpcHandler>>> {
        self.handler_lookup.read()
    }

    /// Collects the sorted, deduplicated names of the public (non-auth) handlers
    /// directly from `&self`, taking only the inner handler-map lock.
    ///
    /// Used both by `crate::server::jsonrpsee_adapter` (after acquiring an outer
    /// read lock) and by `RpcServer::start_rpc_server` (which already holds the
    /// outer write lock and therefore cannot acquire the outer read lock).
    pub fn public_method_names(&self) -> Vec<String> {
        self.method_names(false)
    }

    /// Collects the sorted, deduplicated names of handlers exposed by the
    /// configured transport. Protected methods are only exposed when complete
    /// RPC credentials are configured; the transport middleware still enforces
    /// Basic auth before those handlers execute.
    pub fn transport_method_names(&self) -> Vec<String> {
        self.method_names(self.rpc_auth_configured())
    }

    /// Return whether complete HTTP Basic RPC credentials are configured.
    #[must_use]
    pub fn rpc_auth_configured(&self) -> bool {
        !self.settings.rpc_user.trim().is_empty() && !self.settings.rpc_pass.trim().is_empty()
    }

    fn method_names(&self, include_protected: bool) -> Vec<String> {
        let handlers = self.handlers_guard();
        let mut methods = handlers
            .values()
            .filter(|handler| include_protected || !handler.descriptor().requires_auth())
            .map(|handler| handler.descriptor().name.clone())
            .collect::<Vec<_>>();
        methods.sort_unstable();
        methods.dedup();
        methods
    }

    fn rpc_auth_credentials(&self) -> Result<Option<Arc<http_policy::RpcBasicAuth>>, &'static str> {
        http_policy::auth_credentials_from_settings(&self.settings)
            .map(|credentials| credentials.map(Arc::new))
    }
}

fn rate_limiter_from_settings(settings: &RpcServerConfig) -> GovernorRateLimiter {
    GovernorRateLimiter::new(RateLimitConfig {
        max_rps: settings.max_requests_per_second,
        burst: settings.rate_limit_burst,
    })
}

fn global_rate_limit_key() -> IpAddr {
    IpAddr::V4(Ipv4Addr::UNSPECIFIED)
}
