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
//! - `lifecycle`: jsonrpsee startup, shutdown, and purge-task wiring.

mod http_policy;
mod lifecycle;

use parking_lot::{Mutex, RwLock, RwLockReadGuard};
use prometheus::Counter;
use serde_json::Value;
use std::sync::LazyLock;
use tokio::{sync::oneshot, task::JoinHandle};

use tracing::warn;
use uuid::Uuid;

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Weak};
use std::time::Duration;

use super::middleware::{GovernorRateLimiter, RateLimitCheckResult, RateLimitConfig};
use super::node_context::NodeContext;
use super::rpc_error::RpcError;
use super::rpc_remote_ledger::RemoteLedgerRpcClient;
use super::rpc_server_settings::RpcServerConfig;
use super::session::Session;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_method_attribute::RpcMethodDescriptor;
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
    system: Arc<NodeContext>,
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
    /// Create an RPC server with the given node context and server settings.
    pub fn new(system: Arc<NodeContext>, settings: RpcServerConfig) -> Self {
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

    /// Return the node context served by this RPC instance.
    #[must_use]
    pub fn system(&self) -> Arc<NodeContext> {
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
