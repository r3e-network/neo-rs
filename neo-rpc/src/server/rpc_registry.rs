use super::rpc_server::RpcServer;
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};

pub static SERVERS: Lazy<RwLock<HashMap<u32, Arc<RwLock<RpcServer>>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

pub fn remove_server(network: u32) {
    if SERVERS.write().remove(&network).is_some() {
        info!("Removed RPC server for network {}", network);
    }
}

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

pub fn get_server(network: u32) -> Option<Arc<RwLock<RpcServer>>> {
    SERVERS.read().get(&network).cloned()
}
