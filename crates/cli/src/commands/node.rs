use super::{not_implemented, CommandResult};

/// Node lifecycle commands (`MainService.Node`).
pub struct NodeCommands;

impl NodeCommands {
    pub fn start_node(&self) -> CommandResult {
        not_implemented("start node")
    }
}
