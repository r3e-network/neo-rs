use super::{not_implemented, CommandResult};

/// Commands related to block inspection (see `Neo.CLI/CLI/MainService.Block.cs`).
pub struct BlockCommands;

impl BlockCommands {
    pub fn show_block(&self, _index_or_hash: &str) -> CommandResult {
        not_implemented("show block")
    }
}
