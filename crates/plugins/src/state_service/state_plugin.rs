use super::StateServiceSettings;
use neo_core::NeoSystem;
use neo_extensions::error::{ExtensionError, ExtensionResult};
use neo_extensions::plugin::{Plugin, PluginBase, PluginCategory, PluginContext, PluginEvent, PluginInfo};
use serde_json::Value as JsonValue;
use std::sync::Arc;
use tracing::{info, warn};

/// Minimal StateService plugin scaffold mirroring C# structure.
pub struct StateServicePlugin {
    base: PluginBase,
    settings: StateServiceSettings,
}

impl StateServicePlugin {
    pub fn new() -> Self {
        Self::with_settings(StateServiceSettings::default())
    }

    pub fn with_settings(settings: StateServiceSettings) -> Self {
        let info = PluginInfo {
            name: "StateService".to_string(),
            version: "1.0.0".to_string(),
            description: "State root synchronisation plugin".to_string(),
            author: "Neo Project".to_string(),
            dependencies: vec![],
            min_neo_version: "3.6.0".to_string(),
            category: PluginCategory::Storage,
            priority: 0,
        };

        Self {
            base: PluginBase::new(info),
            settings,
        }
    }

    fn load_settings(&mut self, config: Option<JsonValue>) {
        if let Some(value) = config {
            self.settings = StateServiceSettings::from_config(&value);
        }
    }
}

#[async_trait::async_trait]
impl Plugin for StateServicePlugin {
    fn info(&self) -> &PluginInfo {
        self.base.info()
    }

    async fn initialize(&mut self, context: &PluginContext) -> ExtensionResult<()> {
        if let Err(err) = self.base.ensure_directories() {
            warn!("StateService: unable to create plugin directory: {}", err);
        }

        let config_path = context.config_dir.join("StateService.json");
        let config = match tokio::fs::read_to_string(&config_path).await {
            Ok(contents) if !contents.trim().is_empty() => {
                let value: JsonValue = serde_json::from_str(&contents)
                    .map_err(|err| ExtensionError::invalid_config(err.to_string()))?;
                Some(value)
            }
            Ok(_) => None,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => return Err(err.into()),
        };

        self.load_settings(config);
        Ok(())
    }

    async fn start(&mut self) -> ExtensionResult<()> {
        info!("StateService plugin ready");
        Ok(())
    }

    async fn stop(&mut self) -> ExtensionResult<()> {
        info!("StateService plugin stopping");
        Ok(())
    }

    async fn handle_event(&mut self, event: &PluginEvent) -> ExtensionResult<()> {
        match event {
            PluginEvent::NodeStarted { system } => {
                match Arc::downcast::<NeoSystem>(system.clone()) {
                    Ok(system) => {
                        if system.settings().network != self.settings.network() {
                            warn!("StateService: network mismatch; plugin inactive");
                        }
                    }
                    Err(_) => warn!(
                        "StateService: NodeStarted payload was not a NeoSystem instance"
                    ),
                }
                Ok(())
            }
            PluginEvent::ServiceAdded { .. } => Ok(()),
            _ => Ok(()),
        }
    }
}

neo_extensions::register_plugin!(StateServicePlugin);
