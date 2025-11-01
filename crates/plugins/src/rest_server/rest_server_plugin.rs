// Copyright (C) 2015-2025 The Neo Project.
//
// RestServerPlugin mirrors the behaviour of the C# Neo.Plugins.RestServer
// implementation while following idiomatic Rust patterns.

use crate::rest_server::rest_server_settings::RestServerSettings;
use crate::rest_server::rest_web_server::RestWebServer;
use async_trait::async_trait;
use neo_core::network::p2p::local_node::LocalNode;
use neo_core::NeoSystem;
use neo_extensions::error::{ExtensionError, ExtensionResult};
use neo_extensions::plugin::{
    Plugin, PluginBase, PluginCategory, PluginContext, PluginEvent, PluginInfo,
};
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, warn};

/// Static access to the currently running Neo system and local node—matching
/// the static properties on the C# plugin.
pub struct RestServerGlobals;

static NEO_SYSTEM: Lazy<RwLock<Option<Arc<NeoSystem>>>> = Lazy::new(|| RwLock::new(None));
static LOCAL_NODE: Lazy<RwLock<Option<Arc<LocalNode>>>> = Lazy::new(|| RwLock::new(None));

impl RestServerGlobals {
    pub fn neo_system() -> Option<Arc<NeoSystem>> {
        NEO_SYSTEM.read().clone()
    }

    pub fn local_node() -> Option<Arc<LocalNode>> {
        LOCAL_NODE.read().clone()
    }

    pub fn set_neo_system(system: Arc<NeoSystem>) {
        *NEO_SYSTEM.write() = Some(system);
    }

    pub fn set_local_node(node: Arc<LocalNode>) {
        *LOCAL_NODE.write() = Some(node);
    }

    pub fn clear() {
        *NEO_SYSTEM.write() = None;
        *LOCAL_NODE.write() = None;
    }
}

/// Rust port of the C# `RestServerPlugin` class.
pub struct RestServerPlugin {
    base: PluginBase,
    settings: RestServerSettings,
    server: Option<RestWebServer>,
}

impl RestServerPlugin {
    pub fn new() -> Self {
        let info = PluginInfo {
            name: "RestServer".to_string(),
            version: "1.0.0".to_string(),
            description: "Enables REST Web Services for the node".to_string(),
            author: "Neo Project".to_string(),
            dependencies: vec![],
            min_neo_version: "3.6.0".to_string(),
            category: PluginCategory::Rpc,
            priority: 0,
        };

        let base = PluginBase::new(info);

        Self {
            base,
            settings: RestServerSettings::default(),
            server: None,
        }
    }

    fn load_settings(&mut self, config: Option<Value>) {
        RestServerSettings::load(config.as_ref());
        self.settings = RestServerSettings::current();
    }

    fn configure_from_path(&mut self, path: &PathBuf) -> ExtensionResult<()> {
        if let Err(err) = self.base.ensure_directories() {
            warn!("RestServer: unable to create plugin directory: {}", err);
        }
        match fs::read_to_string(path) {
            Ok(contents) => {
                if contents.trim().is_empty() {
                    self.load_settings(None);
                } else {
                    let value: Value = serde_json::from_str(&contents)
                        .map_err(|err| ExtensionError::invalid_config(err.to_string()))?;
                    self.load_settings(Some(value));
                }
                Ok(())
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                self.load_settings(None);
                Ok(())
            }
            Err(err) => Err(ExtensionError::invalid_config(err.to_string())),
        }
    }

    fn on_system_loaded(&mut self, system: Arc<NeoSystem>) -> ExtensionResult<()> {
        if self.settings.enable_cors
            && self.settings.enable_basic_authentication
            && self.settings.allow_origins.is_empty()
        {
            warn!("RestServer: CORS is misconfigured!");
            warn!("You have EnableCors and EnableBasicAuthentication enabled but AllowOrigins is empty in RestServer.json.");
            warn!(
                "Example: \"AllowOrigins\": [\"http://{}:{}\"]",
                self.settings.bind_address, self.settings.port
            );
        }

        if system.settings().network == self.settings.network {
            RestServerGlobals::set_neo_system(system.clone());

            match system.get_service::<LocalNode>() {
                Ok(Some(node)) => RestServerGlobals::set_local_node(node),
                Ok(None) => warn!("RestServer: LocalNode service not registered in NeoSystem"),
                Err(error) => warn!(
                    "RestServer: unable to resolve LocalNode service from NeoSystem: {}",
                    error
                ),
            }
        } else {
            RestServerGlobals::clear();
        }

        let mut server = RestWebServer::new();
        server.start();
        self.server = Some(server);
        Ok(())
    }

    fn stop_server(&mut self) {
        if let Some(server) = &mut self.server {
            server.stop();
        }
        self.server = None;
        RestServerGlobals::clear();
    }
}

#[async_trait]
impl Plugin for RestServerPlugin {
    fn info(&self) -> &PluginInfo {
        self.base.info()
    }

    async fn initialize(&mut self, context: &PluginContext) -> ExtensionResult<()> {
        let path = context.config_dir.join("RestServer.json");
        self.configure_from_path(&path)
    }

    async fn start(&mut self) -> ExtensionResult<()> {
        info!("RestServer plugin ready");
        Ok(())
    }

    async fn stop(&mut self) -> ExtensionResult<()> {
        self.stop_server();
        Ok(())
    }

    async fn handle_event(&mut self, event: &PluginEvent) -> ExtensionResult<()> {
        match event {
            PluginEvent::NodeStarted { system } => {
                match Arc::downcast::<NeoSystem>(system.clone()) {
                    Ok(neo_system) => self.on_system_loaded(neo_system),
                    Err(_) => {
                        warn!("RestServer: NodeStarted payload was not a NeoSystem instance");
                        Ok(())
                    }
                }
            }
            PluginEvent::NodeStopping => {
                self.stop_server();
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

neo_extensions::register_plugin!(RestServerPlugin);
