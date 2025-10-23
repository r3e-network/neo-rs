//! Application Logs plugin wiring.

use crate::application_logs::log_reader::LogReader;
use crate::application_logs::settings::ApplicationLogsSettings;
use neo_core::NeoSystem;
use neo_extensions::error::{ExtensionError, ExtensionResult};
use neo_extensions::plugin::{Plugin, PluginBase, PluginCategory, PluginContext, PluginEvent, PluginInfo};
use serde_json::Value as JsonValue;
use std::io::ErrorKind;
use std::sync::Arc;
use tracing::warn;
use tokio::fs;

/// Runtime implementation of the Application Logs plugin.
pub struct ApplicationLogsPlugin {
    base: PluginBase,
    settings: ApplicationLogsSettings,
    log_reader: Option<LogReader>,
}

impl ApplicationLogsPlugin {
    /// Creates a new plugin instance using default settings.
    pub fn new() -> Self {
        Self::with_settings(ApplicationLogsSettings::default())
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
        }
    }

    fn ensure_reader(&mut self) {
        if self.log_reader.is_none() {
            self.log_reader = Some(LogReader::new(self.settings.clone()));
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
            warn!("ApplicationLogs: unable to create plugin directory: {}", err);
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
        if let Some(reader) = &mut self.log_reader {
            reader.configure(config_value);
        }

        Ok(())
    }

    async fn start(&mut self) -> ExtensionResult<()> {
        self.ensure_reader();
        Ok(())
    }

    async fn stop(&mut self) -> ExtensionResult<()> {
        if let Some(reader) = &mut self.log_reader {
            reader.dispose();
        }
        self.log_reader = None;
        Ok(())
    }

    async fn handle_event(&mut self, event: &PluginEvent) -> ExtensionResult<()> {
        if let PluginEvent::NodeStarted { system } = event {
                match Arc::downcast::<NeoSystem>(system.clone()) {
                    Ok(neo_system) => {
                        self.ensure_reader();
                        if let Some(reader) = &mut self.log_reader {
                            reader.on_system_loaded(neo_system);
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
        if let Some(reader) = &mut self.log_reader {
            reader.configure(Some(config));
        }
        Ok(())
    }
}

neo_extensions::register_plugin!(ApplicationLogsPlugin);
