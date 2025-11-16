use super::{not_implemented, CommandResult};

/// Miscellaneous utilities (`MainService.Tools`).
pub struct ToolCommands;

impl ToolCommands {
    pub fn calculate(&self, _expression: &str) -> CommandResult {
        not_implemented("calc")
    }
}
