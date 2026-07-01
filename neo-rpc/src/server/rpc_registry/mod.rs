//! # neo-rpc::server::rpc_registry
//!
//! RPC server registry and method table.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `rpc_registry`: process-local RPC server registry and lookup API.

use super::rpc_server::RpcServer;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::LazyLock;
use tracing::{info, warn};

/// Process-local RPC server registry keyed by Neo network magic.
pub static SERVERS: LazyLock<RwLock<HashMap<u32, Arc<RwLock<RpcServer>>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Helpers for registering and looking up RPC server instances.
pub struct ServerRegistry;

impl ServerRegistry {
    /// Remove the RPC server registered for `network`.
    pub fn remove_server(network: u32) {
        if SERVERS.write().remove(&network).is_some() {
            info!("Removed RPC server for network {}", network);
        }
    }

    /// Register an RPC server for `network`, replacing any existing instance.
    pub fn register_server(network: u32, server: Arc<RwLock<RpcServer>>) {
        let mut guard = SERVERS.write();
        if let Some(previous) = guard.insert(network, Arc::clone(&server)) {
            warn!(
                "Replacing existing RPC server instance for network {}",
                network
            );
            if let Some(mut previous_guard) = previous.try_write() {
                previous_guard.dispose();
            }
        }
    }

    /// Return the registered RPC server for `network`.
    pub fn get_server(network: u32) -> Option<Arc<RwLock<RpcServer>>> {
        SERVERS.read().get(&network).cloned()
    }
}
