use super::{not_implemented, CommandResult};

/// Blockchain state commands (mirrors `MainService.Blockchain`).
pub struct BlockchainCommands;

impl BlockchainCommands {
    pub fn show_state(&self) -> CommandResult {
        not_implemented("show state")
    }
}
