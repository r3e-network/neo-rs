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
//! - `handler`: RPC callback and handler descriptor bindings.
//! - `http_policy`: HTTP policy helpers for the RPC server.
//! - `lifecycle`: jsonrpsee startup, shutdown, and purge-task wiring.
//! - `metrics`: Prometheus counters for RPC request/error totals.
//! - `rate_limit`: RPC-server rate-limit configuration and error mapping.
//! - `registry`: RPC handler registration and transport method projection.
//! - `sessions`: invoke-session storage and expiration helpers.

mod handler;
mod http_policy;
mod lifecycle;
mod metrics;
mod rate_limit;
mod registry;
mod sessions;

use parking_lot::RwLock;
use tokio::{sync::oneshot, task::JoinHandle};

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use super::middleware::GovernorRateLimiter;
use super::node_context::NodeContext;
use super::rpc_error::RpcError;
use super::rpc_remote_ledger::RemoteLedgerRpcClient;
use super::rpc_server_settings::RpcServerConfig;
use neo_wallets::Wallet;

pub use handler::{RpcCallback, RpcHandler};
pub(crate) use handler::{protected_rpc_handler, rpc_handler};
pub use metrics::{RPC_ERR_TOTAL, RPC_REQ_TOTAL};
use rate_limit::rate_limiter_from_settings;

/// Type alias for wallet change callback to reduce complexity.
pub type WalletChangeCallback = Arc<dyn Fn(Option<Arc<dyn Wallet>>) + Send + Sync>;

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
    sessions: sessions::SessionStore,
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
            sessions: sessions::new_session_store(),
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

    /// Return whether complete HTTP Basic RPC credentials are configured.
    #[must_use]
    pub fn rpc_auth_configured(&self) -> bool {
        !self.settings.rpc_user.trim().is_empty() && !self.settings.rpc_pass.trim().is_empty()
    }

    fn rpc_auth_credentials(&self) -> Result<Option<Arc<http_policy::RpcBasicAuth>>, &'static str> {
        http_policy::auth_credentials_from_settings(&self.settings)
            .map(|credentials| credentials.map(Arc::new))
    }
}
