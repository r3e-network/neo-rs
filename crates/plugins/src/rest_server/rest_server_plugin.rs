//! Port scaffold for `RestServerPlugin.cs`.
//! The final implementation will expose the same REST surface as the C#[derive(Debug)]node.

use std::path::PathBuf;

use crate::Plugin;
use async_trait::async_trait;
use neo_extensions::plugin::{PluginCategory, PluginContext, PluginEvent, PluginInfo};
use neo_extensions::{ExtensionError, ExtensionResult};

use super::{rest_server_settings::RestServerSettings, rest_web_server::RestWebServer};

#[derive(Debug, Default)]
pub struct RestServerPlugin {
    info: PluginInfo,
    config_path: Option<PathBuf>,
    settings: RestServerSettings,
    server: Option<RestWebServer>,
}

impl RestServerPlugin {
    pub fn new() -> Self {
        Self {
            info: PluginInfo {
                name: "RestServer".to_string(),
                version: "0.4.0".to_string(),
                author: "Neo Project".to_string(),
                description: "Enables REST web services for the node".to_string(),
                category: PluginCategory::Rpc,
                dependencies: Vec::new(),
                min_neo_version: "3.6.0".to_string(),
                priority: 0,
            },
            config_path: None,
            settings: RestServerSettings::default(),
            server: None,
        }
    }
}

#[async_trait]
impl Plugin for RestServerPlugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }

    async fn initialize(&mut self, context: &PluginContext) -> ExtensionResult<()> {
        let config_path = context.config_dir.join("RestServer.json");
        let settings = RestServerSettings::load_from_path(&config_path)
            .map_err(|err| ExtensionError::invalid_config(err.to_string()))?;

        self.config_path = Some(config_path);
        self.settings = settings;
        Ok(())
    }

    async fn start(&mut self) -> ExtensionResult<()> {
        let server = RestWebServer::new(self.settings.clone());
        server.start().await?;
        self.server = Some(server);
        Ok(())
    }

    async fn stop(&mut self) -> ExtensionResult<()> {
        if let Some(server) = &self.server {
            server.stop().await?;
        }
        self.server = None;
        Ok(())
    }

    async fn handle_event(&mut self, _event: &PluginEvent) -> ExtensionResult<()> {
        // TODO: forward blockchain events to REST push channels.
        Ok(())
    }
}
