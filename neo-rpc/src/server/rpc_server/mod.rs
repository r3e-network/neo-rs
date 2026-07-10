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
//! - `state`: server construction, settings, upstream, and WebSocket accessors.
//! - `wallet`: active-wallet state and wallet-change callbacks.

mod handler;
mod http_policy;
mod lifecycle;
mod metrics;
mod rate_limit;
mod registry;
mod sessions;
mod state;
mod wallet;

use parking_lot::RwLock;
use tokio::{sync::oneshot, task::JoinHandle};

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use super::middleware::GovernorRateLimiter;
use super::node_context::NodeContext;
use super::rpc_remote_ledger::RemoteLedgerRpcClient;
use super::rpc_server_settings::RpcServerConfig;

type RpcOracleService = neo_oracle_service::OracleService<NodeContext>;

pub use handler::{RpcCallback, RpcHandler};
pub(crate) use handler::{protected_rpc_handler, rpc_handler};
pub use metrics::{RPC_ERR_TOTAL, RPC_REQ_TOTAL};

/// JSON-RPC server for a Neo node.
pub struct RpcServer {
    system: Arc<NodeContext>,
    settings: RpcServerConfig,
    handler_lookup: Arc<RwLock<HashMap<String, Arc<RpcHandler>>>>,
    started: bool,
    wallet: wallet::WalletHandle,
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
    oracle_service: Option<Arc<RpcOracleService>>,
}
