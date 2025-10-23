use super::oracle_service::OracleService;
use super::settings::OracleServiceSettings;
use neo_core::NeoSystem;
use neo_extensions::error::{ExtensionError, ExtensionResult};
use neo_extensions::plugin::{Plugin, PluginBase, PluginCategory, PluginContext, PluginEvent, PluginInfo};
use serde_json::Value as JsonValue;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};

/// Oracle service plugin aligning with the C# OracleService plugin lifecycle.
pub struct OracleServicePlugin {
    base: PluginBase,
    settings: OracleServiceSettings,
    service: Option<Arc<Mutex<OracleService>>>,
}

impl OracleServicePlugin {
    pub fn new() -> Self {
        Self::with_settings(OracleServiceSettings::default())
    }

    pub fn with_settings(settings: OracleServiceSettings) -> Self {
        let info = PluginInfo {
            name: "OracleService".to_string(),
            version: "1.0.0".to_string(),
            description: "Oracle service plugin for Neo".to_string(),
            author: "Neo Project".to_string(),
            dependencies: vec![],
            min_neo_version: "3.6.0".to_string(),
            category: PluginCategory::Utility,
            priority: 0,
        };

        Self {
            base: PluginBase::new(info),
            settings,
            service: None,
        }
    }

    fn ensure_service(&mut self) {
        if self.service.is_none() {
            let service = OracleService::new(self.settings.clone());
            self.service = Some(Arc::new(Mutex::new(service)));
        }
    }

    async fn start_service(&mut self) -> ExtensionResult<()> {
        if !self.settings.enabled {
            return Ok(());
        }
        self.ensure_service();
        if let Some(service) = &self.service {
            let mut guard = service.lock().await;
            guard
                .start()
                .await
                .map_err(|err| ExtensionError::operation_failed(err))?
        }
        Ok(())
    }

    async fn stop_service(&mut self) -> ExtensionResult<()> {
        if let Some(service) = &self.service {
            let mut guard = service.lock().await;
            guard
                .stop()
                .await
                .map_err(|err| ExtensionError::operation_failed(err))?
        }
        Ok(())
    }

    fn load_settings(&mut self, config: Option<JsonValue>) {
        if let Some(value) = config {
            if let Ok(parsed) = serde_json::from_value::<OracleServiceSettings>(value) {
                self.settings = parsed;
            }
        }
    }
}

#[async_trait::async_trait]
impl Plugin for OracleServicePlugin {
    fn info(&self) -> &PluginInfo {
        self.base.info()
    }

    async fn initialize(&mut self, context: &PluginContext) -> ExtensionResult<()> {
        if let Err(err) = self.base.ensure_directories() {
            warn!("OracleService: unable to create plugin directory: {}", err);
        }

        let config_path = context.config_dir.join("OracleService.json");
        let config = match tokio::fs::read_to_string(&config_path).await {
            Ok(contents) if !contents.trim().is_empty() => {
                serde_json::from_str(&contents)
                    .map_err(|err| ExtensionError::invalid_config(err.to_string()))
                    .map(Some)?
            }
            Ok(_) => None,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => return Err(err.into()),
        };

        self.load_settings(config);
        Ok(())
    }

    async fn start(&mut self) -> ExtensionResult<()> {
        info!("OracleService plugin ready");
        self.start_service().await
    }

    async fn stop(&mut self) -> ExtensionResult<()> {
        self.stop_service().await
    }

    async fn handle_event(&mut self, event: &PluginEvent) -> ExtensionResult<()> {
        match event {
            PluginEvent::NodeStarted { system } => {
                match Arc::downcast::<NeoSystem>(system.clone()) {
                    Ok(_system) => {
                        // Oracle plugin currently does not require NeoSystem specific data
                        self.start_service().await?;
                    }
                    Err(_) => warn!("OracleService: NodeStarted payload was not a NeoSystem instance"),
                }
                Ok(())
            }
            PluginEvent::NodeStopping => self.stop_service().await,
            _ => Ok(()),
        }
    }
}

neo_extensions::register_plugin!(OracleServicePlugin);
