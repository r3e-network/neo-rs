//! RPC server construction and direct state accessors.
//!
//! The root module owns the `RpcServer` field layout. This module owns the
//! small, direct policies for constructing that layout and exposing settings,
//! upstream ledger RPC, WebSocket bridge, and authentication state.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use super::http_policy;
use super::rate_limit::rate_limiter_from_settings;
use super::{RpcServer, sessions, wallet};
use crate::server::node_context::NodeContext;
use crate::server::rpc_error::RpcError;
use crate::server::rpc_remote_ledger::RemoteLedgerRpcClient;
use crate::server::rpc_server_settings::RpcServerConfig;

impl RpcServer {
    /// Create an RPC server with the given node context and server settings.
    pub fn new(system: Arc<NodeContext>, settings: RpcServerConfig) -> Self {
        let rate_limiter = Arc::new(rate_limiter_from_settings(&settings));
        Self {
            system,
            settings,
            handler_lookup: Arc::new(RwLock::new(HashMap::new())),
            started: false,
            wallet: wallet::new_wallet_handle(),
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

    /// Enable WebSocket subscriptions.
    ///
    /// Creates and returns an event bridge that can be used to push events
    /// to connected WebSocket clients. Call this before `start_rpc_server`.
    pub fn enable_websocket(&mut self, capacity: usize) -> Arc<super::super::ws::WsEventBridge> {
        let bridge = Arc::new(super::super::ws::WsEventBridge::new(capacity));
        self.ws_bridge = Some(Arc::clone(&bridge));
        bridge
    }

    /// Return the WebSocket event bridge if enabled.
    #[must_use]
    pub fn ws_bridge(&self) -> Option<Arc<super::super::ws::WsEventBridge>> {
        self.ws_bridge.clone()
    }

    /// Return whether WebSocket subscriptions are enabled.
    #[must_use]
    pub const fn is_websocket_enabled(&self) -> bool {
        self.ws_bridge.is_some()
    }

    /// Return whether the RPC server has been started.
    #[must_use]
    pub const fn is_started(&self) -> bool {
        self.started
    }

    /// Return whether complete HTTP Basic RPC credentials are configured.
    #[must_use]
    pub fn rpc_auth_configured(&self) -> bool {
        !self.settings.rpc_user.trim().is_empty() && !self.settings.rpc_pass.trim().is_empty()
    }

    pub(super) fn rpc_auth_credentials(
        &self,
    ) -> Result<Option<Arc<http_policy::RpcBasicAuth>>, &'static str> {
        http_policy::auth_credentials_from_settings(&self.settings)
            .map(|credentials| credentials.map(Arc::new))
    }
}
