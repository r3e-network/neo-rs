use neo_core::extensions::plugin::{
    Plugin, PluginBase, PluginCategory, PluginContext, PluginEvent, PluginInfo,
};

/// Minimal placeholder plugin for `tokens-tracker`.
///
/// This keeps the crate compiling while the full TokensTracker port is in
/// progress. The plugin does not register any RPC methods or handle chain
/// events beyond startup/shutdown bookkeeping.
pub struct TokensTrackerPlugin {
    base: PluginBase,
}

impl TokensTrackerPlugin {
    pub fn new() -> Self {
        Self {
            base: PluginBase::new(PluginInfo {
                name: "TokensTracker".to_string(),
                version: "0.0.0".to_string(),
                description: "Token balance/transfer tracking (stub)".to_string(),
                author: "Neo Project".to_string(),
                dependencies: vec![],
                min_neo_version: "3.6.0".to_string(),
                category: PluginCategory::Utility,
                priority: 0,
            }),
        }
    }
}

impl Default for TokensTrackerPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Plugin for TokensTrackerPlugin {
    fn info(&self) -> &PluginInfo {
        self.base.info()
    }

    async fn initialize(
        &mut self,
        _context: &PluginContext,
    ) -> neo_core::extensions::error::ExtensionResult<()> {
        Ok(())
    }

    async fn start(&mut self) -> neo_core::extensions::error::ExtensionResult<()> {
        Ok(())
    }

    async fn stop(&mut self) -> neo_core::extensions::error::ExtensionResult<()> {
        Ok(())
    }

    async fn handle_event(
        &mut self,
        _event: &PluginEvent,
    ) -> neo_core::extensions::error::ExtensionResult<()> {
        Ok(())
    }
}
