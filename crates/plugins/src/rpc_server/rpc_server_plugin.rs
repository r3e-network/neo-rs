use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use neo_core::{i_event_handlers::IWalletChangedHandler, wallets::Wallet, NeoSystem};
use neo_extensions::error::{ExtensionError, ExtensionResult};
use neo_extensions::plugin::{
    Plugin, PluginBase, PluginCategory, PluginContext, PluginEvent, PluginInfo,
};
use parking_lot::RwLock;
use serde_json::Value;
use tracing::{info, warn};

use super::rcp_server_settings::{RpcServerConfig, RpcServerSettings};
use super::rpc_server::{
    add_pending_handler, get_server, register_server, remove_server, take_pending_handlers,
    RpcHandler, RpcServer, SERVERS,
};
use super::rpc_server_blockchain::RpcServerBlockchain;
use super::rpc_server_node::RpcServerNode;
use super::rpc_server_smart_contract::RpcServerSmartContract;
use super::rpc_server_state::RpcServerState;
use super::rpc_server_utilities::RpcServerUtilities;
use super::rpc_server_wallet::RpcServerWallet;

pub struct RpcServerPlugin {
    base: PluginBase,
    settings: RpcServerSettings,
    wallet_handlers: HashSet<u32>,
}

impl RpcServerPlugin {
    pub fn new() -> Self {
        let info = PluginInfo {
            name: "RpcServer".to_string(),
            version: "1.0.0".to_string(),
            description: "Enables RPC for the node".to_string(),
            author: "Neo Project".to_string(),
            dependencies: vec![],
            min_neo_version: "3.6.0".to_string(),
            category: PluginCategory::Rpc,
            priority: 0,
        };

        Self {
            base: PluginBase::new(info),
            settings: RpcServerSettings::default(),
            wallet_handlers: HashSet::new(),
        }
    }

    fn load_settings_from_file(path: &PathBuf) -> ExtensionResult<()> {
        match fs::read_to_string(path) {
            Ok(contents) => {
                if contents.trim().is_empty() {
                    RpcServerSettings::load(None);
                } else {
                    let value: Value = serde_json::from_str(&contents)
                        .map_err(|err| ExtensionError::invalid_config(err.to_string()))?;
                    RpcServerSettings::load(Some(&value));
                }
                Ok(())
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                RpcServerSettings::load(None);
                Ok(())
            }
            Err(err) => Err(ExtensionError::invalid_config(err.to_string())),
        }
    }

    fn ensure_server_for_network(&self, system: Arc<NeoSystem>, config: RpcServerConfig) {
        let network = config.network;
        if let Some(server_arc) = get_server(network) {
            if let Some(mut server) = server_arc.try_write() {
                server.update_settings(config);
                if !server.is_started() {
                    server.start_rpc_server(Arc::downgrade(&server_arc));
                }
            }
            return;
        }

        let mut server = RpcServer::new(system.clone(), config.clone());
        server.register_handlers(RpcServerBlockchain::register_handlers());
        server.register_handlers(RpcServerNode::register_handlers());
        server.register_handlers(RpcServerState::register_handlers());
        server.register_handlers(RpcServerSmartContract::register_handlers());
        server.register_handlers(RpcServerWallet::register_handlers());
        server.register_handlers(RpcServerUtilities::register_handlers());
        let pending = take_pending_handlers(network);
        if !pending.is_empty() {
            server.register_handlers(pending);
        }
        let server_arc = Arc::new(RwLock::new(server));
        {
            let weak = Arc::downgrade(&server_arc);
            if let Some(mut guard) = server_arc.try_write() {
                guard.start_rpc_server(weak);
            }
        }
        register_server(network, Arc::clone(&server_arc));

        // Expose server for readiness/inspection via service registry.
        let name = format!("RpcServer:{network}");
        if let Err(err) = system.add_named_service::<RwLock<RpcServer>, _>(name, server_arc.clone())
        {
            warn!("RpcServer: failed to register server service: {}", err);
        }
    }

    fn stop_server_for_network(&self, network: u32) {
        if let Some(server_arc) = get_server(network) {
            if let Some(mut server) = server_arc.try_write() {
                server.dispose();
            }
            remove_server(network);
        }
    }

    pub fn register_methods(handler: RpcHandler, network: u32) {
        if let Some(server_arc) = get_server(network) {
            if let Some(mut server) = server_arc.try_write() {
                server.register_method(handler);
                return;
            }
        }

        add_pending_handler(network, handler);
    }
}

#[async_trait]
impl Plugin for RpcServerPlugin {
    fn info(&self) -> &PluginInfo {
        self.base.info()
    }

    async fn initialize(&mut self, context: &PluginContext) -> ExtensionResult<()> {
        if let Err(err) = self.base.ensure_directories() {
            warn!("RpcServer: unable to create plugin directory: {}", err);
        }
        let path = context.config_dir.join("RpcServer.json");
        Self::load_settings_from_file(&path)?;
        self.settings = RpcServerSettings::current();
        Ok(())
    }

    async fn start(&mut self) -> ExtensionResult<()> {
        info!("RpcServer plugin started");
        Ok(())
    }

    async fn stop(&mut self) -> ExtensionResult<()> {
        info!("RpcServer plugin stopping");
        Ok(())
    }

    async fn handle_event(&mut self, event: &PluginEvent) -> ExtensionResult<()> {
        match event {
            PluginEvent::NodeStarted { system } => {
                let system = match Arc::downcast::<NeoSystem>(system.clone()) {
                    Ok(system) => system,
                    Err(_) => {
                        warn!("RpcServer: NodeStarted payload was not a NeoSystem instance");
                        return Ok(());
                    }
                };

                let network = system.settings().network;
                let config = match self.settings.server_for_network(network) {
                    Some(cfg) => cfg,
                    None => {
                        warn!("RpcServer: no configuration found for network {}", network);
                        return Ok(());
                    }
                };
                self.ensure_server_for_network(system.clone(), config);
                if !self.wallet_handlers.contains(&network) {
                    let handler = Arc::new(RpcServerWalletHandler { network });
                    match system.register_wallet_changed_handler(handler) {
                        Ok(()) => {
                            self.wallet_handlers.insert(network);
                        }
                        Err(err) => warn!(
                            "RpcServer: failed to register wallet handler for network {}: {}",
                            network, err
                        ),
                    }
                }
                Ok(())
            }
            PluginEvent::NodeStopping => {
                let networks: Vec<u32> = SERVERS.read().keys().copied().collect();
                for network in networks {
                    self.stop_server_for_network(network);
                    self.wallet_handlers.remove(&network);
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

impl Default for RpcServerPlugin {
    fn default() -> Self {
        Self::new()
    }
}

struct RpcServerWalletHandler {
    network: u32,
}

impl IWalletChangedHandler for RpcServerWalletHandler {
    fn i_wallet_provider_wallet_changed_handler(
        &self,
        _sender: &dyn std::any::Any,
        wallet: Option<Arc<dyn Wallet>>,
    ) {
        if let Some(server_arc) = get_server(self.network) {
            if let Some(server) = server_arc.try_write() {
                server.set_wallet(wallet);
            }
        }
    }
}

neo_extensions::register_plugin!(RpcServerPlugin);
