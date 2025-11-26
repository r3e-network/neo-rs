use super::CommandResult;
use crate::console_service::ConsoleHelper;
use neo_core::{neo_system::NeoSystem, smart_contract::native::NativeRegistry};
use std::sync::Arc;

/// Native contract management (`MainService.Native`).
pub struct NativeCommands {
    system: Arc<NeoSystem>,
}

impl NativeCommands {
    pub fn new(system: Arc<NeoSystem>) -> Self {
        Self { system }
    }

    /// Lists native contracts, mirroring `OnListNativeContract`.
    pub fn list_native_contracts(&self) -> CommandResult {
        let registry = NativeRegistry::new();
        let mut contracts: Vec<_> = registry.contracts().collect();
        if contracts.is_empty() {
            ConsoleHelper::info(["No native contracts registered."]);
            return Ok(());
        }

        contracts.sort_by(|a, b| a.name().cmp(b.name()));
        let height = self.system.current_block_index();
        let settings = self.system.settings();

        for contract in contracts {
            let active = contract.is_active(settings, height);
            let name = format!("\t{:<20}", contract.name());
            let mut details = contract.hash().to_string();
            if !active {
                details.push_str(" not active yet");
            }
            ConsoleHelper::info([name, details]);
        }
        Ok(())
    }
}
