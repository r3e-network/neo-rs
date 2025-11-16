use super::{not_implemented, CommandResult};

/// Peer/network commands (`MainService.Network`).
pub struct NetworkCommands;

impl NetworkCommands {
    pub fn show_node(&self, _identifier: &str) -> CommandResult {
        not_implemented("show node")
    }
}
