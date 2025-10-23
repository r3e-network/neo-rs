use neo_core::sign::ISigner;
use neo_core::NeoSystem;
use neo_extensions::error::ExtensionResult;
use neo_extensions::plugin::{Plugin, PluginCategory, PluginContext, PluginEvent, PluginInfo};
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct DbftSettings {
    pub auto_start: bool,
    pub network: u32,
}

pub struct DBFTPlugin {
    info: PluginInfo,
    _settings: DbftSettings,
}

impl DBFTPlugin {
    pub fn new(settings: DbftSettings) -> Self {
        Self {
            info: PluginInfo {
                name: "DBFT".to_string(),
                version: "0.0.0".to_string(),
                description: "dBFT consensus (stub implementation)".to_string(),
                author: "Neo Project".to_string(),
                dependencies: Vec::new(),
                min_neo_version: "3.0.0".to_string(),
                category: PluginCategory::Consensus,
                priority: 0,
            },
            _settings: settings,
        }
    }
}

#[async_trait::async_trait]
impl Plugin for DBFTPlugin {
    fn info(&self) -> &PluginInfo {
        &self.info
    }

    async fn initialize(&mut self, _context: &PluginContext) -> ExtensionResult<()> {
        Ok(())
    }

    async fn start(&mut self) -> ExtensionResult<()> {
        Ok(())
    }

    async fn stop(&mut self) -> ExtensionResult<()> {
        Ok(())
    }

    async fn handle_event(&mut self, _event: &PluginEvent) -> ExtensionResult<()> {
        Ok(())
    }
}

pub struct ConsensusService;

impl ConsensusService {
    pub fn new(_system: Arc<NeoSystem>, _settings: DbftSettings, _signer: Arc<dyn ISigner>) -> Self {
        Self
    }
}
