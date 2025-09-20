//! Placeholder JSON-RPC server wiring for the networking crate.
//!
//! The full RPC surface is implemented in the dedicated `neo-rpc-server` crate.
//! This module only keeps a minimal façade so older call sites continue to
//! compile while the real RPC wiring is completed elsewhere.

use crate::{NetworkError, NetworkResult as Result};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;

/// Minimal configuration for the stub RPC server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcConfig {
    /// Address the HTTP listener would bind to once implemented.
    pub http_address: SocketAddr,
}

impl Default for RpcConfig {
    fn default() -> Self {
        Self {
            http_address: "127.0.0.1:10332".parse().expect("valid loopback address"),
        }
    }
}

/// Lightweight state holder made available for future extensions.
#[derive(Debug, Clone, Default)]
pub struct RpcState {
    /// Optional handle to the underlying blockchain state once the RPC layer is rebuilt.
    pub blockchain: Option<Arc<neo_ledger::Blockchain>>,
}

/// No-op RPC server façade that keeps the API surface compiling.
#[derive(Debug, Clone)]
pub struct RpcServer {
    config: RpcConfig,
    state: Arc<RpcState>,
}

impl RpcServer {
    /// Creates a new stub server with the provided configuration.
    pub fn new(config: RpcConfig, state: RpcState) -> Self {
        Self {
            config,
            state: Arc::new(state),
        }
    }

    /// Starts the server. The stub simply validates the configuration and returns.
    pub async fn start(&self) -> Result<()> {
        if self.config.http_address.port() == 0 {
            return Err(NetworkError::Rpc {
                method: "start-stub".to_string(),
                code: -1,
                message: "RPC server needs a non-zero port".to_string(),
            });
        }
        Ok(())
    }

    /// Stops the server. The stub has no running tasks so this is a no-op.
    pub async fn stop(&self) {}

    /// Returns a clone of the internal state so callers can wire additional handlers later.
    pub fn state(&self) -> Arc<RpcState> {
        self.state.clone()
    }
}

/// Simple helper to build a server with default configuration and empty state.
pub fn new_stub_server() -> RpcServer {
    RpcServer::new(RpcConfig::default(), RpcState::default())
}
