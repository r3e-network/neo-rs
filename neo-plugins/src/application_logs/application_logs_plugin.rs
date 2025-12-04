//! Application Logs plugin wiring.

use crate::application_logs::log_reader::LogReader;
use crate::application_logs::rpc_handlers::{
    register_log_reader, unregister_log_reader, ApplicationLogsRpcHandlers,
};
use crate::application_logs::settings::ApplicationLogsSettings;
use crate::rpc_server::rpc_server_plugin::RpcServerPlugin;
use neo_core::extensions::error::{ExtensionError, ExtensionResult};
use neo_core::extensions::plugin::{
    Plugin, PluginBase, PluginCategory, PluginContext, PluginEvent, PluginInfo,
};
use neo_core::NeoSystem;
use parking_lot::RwLock;
use serde_json::Value as JsonValue;
use std::collections::HashSet;
use std::io::ErrorKind;
use std::sync::Arc;
use tokio::fs;
use tracing::warn;

/// Runtime implementation of the Application Logs plugin.
pub struct ApplicationLogsPlugin {
    base: PluginBase,
    settings: ApplicationLogsSettings,
    log_reader: Option<Arc<RwLock<LogReader>>>,
    registered_networks: HashSet<u32>,
}

impl ApplicationLogsPlugin {
    /// Creates a new plugin instance using default settings.
    pub fn new() -> Self {
        Self::with_settings(ApplicationLogsSettings::current())
    }

    /// Creates a new plugin instance with the supplied settings.
    pub fn with_settings(settings: ApplicationLogsSettings) -> Self {
        let info = PluginInfo {
            name: "ApplicationLogs".to_string(),
            version: "1.0.0".to_string(),
            description: "Synchronizes VM execution information for inspection".to_string(),
            author: "Neo Project".to_string(),
            dependencies: Vec::new(),
            min_neo_version: "3.0.0".to_string(),
            category: PluginCategory::Utility,
            priority: 0,
        };

        Self {
            base: PluginBase::new(info),
            settings,
            log_reader: None,
            registered_networks: HashSet::new(),
        }
    }

    fn ensure_reader(&mut self) {
        if self.log_reader.is_none() {
            self.log_reader = Some(Arc::new(RwLock::new(LogReader::new(self.settings.clone()))));
        }
    }
}

#[async_trait::async_trait]
impl Plugin for ApplicationLogsPlugin {
    fn info(&self) -> &PluginInfo {
        self.base.info()
    }

    async fn initialize(&mut self, context: &PluginContext) -> ExtensionResult<()> {
        if let Err(err) = self.base.ensure_directories() {
            warn!(
                "ApplicationLogs: unable to create plugin directory: {}",
                err
            );
        }
        let config_path = context.config_dir.join("ApplicationLogs.json");
        let mut config_value: Option<JsonValue> = None;

        match fs::read_to_string(&config_path).await {
            Ok(content) => {
                let value: JsonValue = serde_json::from_str(&content)?;
                ApplicationLogsSettings::load(&value);
                self.settings = ApplicationLogsSettings::from_config(&value);
                config_value = Some(value);
            }
            Err(err) if err.kind() == ErrorKind::NotFound => {
                let value = JsonValue::Null;
                ApplicationLogsSettings::load(&value);
                self.settings = ApplicationLogsSettings::from_config(&value);
            }
            Err(err) => return Err(ExtensionError::IoError(err)),
        }

        self.ensure_reader();
        if let Some(reader_arc) = &self.log_reader {
            reader_arc.write().configure(config_value);
        }

        Ok(())
    }

    async fn start(&mut self) -> ExtensionResult<()> {
        self.ensure_reader();
        Ok(())
    }

    async fn stop(&mut self) -> ExtensionResult<()> {
        // Unregister from all networks
        for network in self.registered_networks.drain() {
            unregister_log_reader(network);
        }
        if let Some(reader_arc) = &self.log_reader {
            reader_arc.write().dispose();
        }
        self.log_reader = None;
        Ok(())
    }

    async fn handle_event(&mut self, event: &PluginEvent) -> ExtensionResult<()> {
        if let PluginEvent::NodeStarted { system } = event {
            match Arc::downcast::<NeoSystem>(system.clone()) {
                Ok(neo_system) => {
                    let network = neo_system.settings().network;
                    self.ensure_reader();
                    if let Some(reader_arc) = &self.log_reader {
                        reader_arc.write().on_system_loaded(neo_system);

                        // Register RPC handlers for this network if not already done
                        if !self.registered_networks.contains(&network) {
                            register_log_reader(network, Arc::clone(reader_arc));
                            for handler in ApplicationLogsRpcHandlers::register_handlers() {
                                RpcServerPlugin::register_methods(handler, network);
                            }
                            self.registered_networks.insert(network);
                        }
                    }
                }
                Err(_) => {
                    warn!("ApplicationLogs: received NodeStarted with unexpected payload");
                }
            }
        }
        Ok(())
    }

    async fn update_config(&mut self, config: JsonValue) -> ExtensionResult<()> {
        ApplicationLogsSettings::load(&config);
        self.settings = ApplicationLogsSettings::from_config(&config);
        if let Some(reader_arc) = &self.log_reader {
            reader_arc.write().configure(Some(config));
        }
        Ok(())
    }
}

impl Default for ApplicationLogsPlugin {
    fn default() -> Self {
        Self::new()
    }
}

neo_core::register_plugin!(ApplicationLogsPlugin);
