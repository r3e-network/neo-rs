use std::sync::Arc;

use neo_core::neo_system::NeoSystem;
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use tracing::{info, warn};

use super::rcp_server_settings::RpcServerConfig;

pub type RpcHandler = Arc<dyn Send + Sync + 'static>;

pub struct RpcServer {
    system: Arc<NeoSystem>,
    settings: RpcServerConfig,
    handlers: Vec<RpcHandler>,
    started: bool,
}

impl RpcServer {
    pub fn new(system: Arc<NeoSystem>, settings: RpcServerConfig) -> Self {
        Self {
            system,
            settings,
            handlers: Vec::new(),
            started: false,
        }
    }

    pub fn settings(&self) -> &RpcServerConfig {
        &self.settings
    }

    pub fn update_settings(&mut self, settings: RpcServerConfig) {
        self.settings = settings;
    }

    pub fn start_rpc_server(&mut self) {
        if self.started {
            return;
        }
        info!(
            "Starting RPC server on {}:{} (network {})",
            self.settings.bind_address, self.settings.port, self.settings.network
        );
        self.started = true;
    }

    pub fn stop_rpc_server(&mut self) {
        if self.started {
            info!("Stopping RPC server for network {}", self.settings.network);
            self.started = false;
        }
    }

    pub fn register_methods(&mut self, handler: RpcHandler) {
        self.handlers.push(handler);
    }

    pub fn register_handlers(&mut self, handlers: Vec<RpcHandler>) {
        for handler in handlers {
            self.register_methods(handler);
        }
    }

    pub fn is_started(&self) -> bool {
        self.started
    }

    pub fn dispose(&mut self) {
        self.stop_rpc_server();
        self.handlers.clear();
    }
}

pub static SERVERS: Lazy<RwLock<std::collections::HashMap<u32, Arc<RwLock<RpcServer>>>>> =
    Lazy::new(|| RwLock::new(std::collections::HashMap::new()));

pub static PENDING_HANDLERS: Lazy<RwLock<std::collections::HashMap<u32, Vec<RpcHandler>>>> =
    Lazy::new(|| RwLock::new(std::collections::HashMap::new()));

pub fn remove_server(network: u32) {
    if SERVERS.write().remove(&network).is_some() {
        info!("Removed RPC server for network {}", network);
    }
}

pub fn add_pending_handler(network: u32, handler: RpcHandler) {
    let mut guard = PENDING_HANDLERS.write();
    guard.entry(network).or_default().push(handler);
}

pub fn take_pending_handlers(network: u32) -> Vec<RpcHandler> {
    PENDING_HANDLERS
        .write()
        .remove(&network)
        .unwrap_or_default()
}

pub fn register_server(network: u32, server: Arc<RwLock<RpcServer>>) {
    let mut guard = SERVERS.write();
    if let Some(previous) = guard.insert(network, Arc::clone(&server)) {
        warn!(
            "Replacing existing RPC server instance for network {}",
            network
        );
        if let Ok(mut previous_guard) = previous.try_write() {
            previous_guard.dispose();
        }
    }
}

pub fn get_server(network: u32) -> Option<Arc<RwLock<RpcServer>>> {
    SERVERS.read().get(&network).cloned()
}
