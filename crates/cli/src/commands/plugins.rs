use super::{not_implemented, CommandResult};

/// Plugin commands (`MainService.Plugins`).
pub struct PluginCommands;

impl PluginCommands {
    pub fn list_plugins(&self) -> CommandResult {
        not_implemented("list plugins")
    }
}
