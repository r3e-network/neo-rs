use super::{not_implemented, CommandResult};

/// Governance commands (`MainService.Vote`).
pub struct VoteCommands;

impl VoteCommands {
    pub fn show_candidates(&self) -> CommandResult {
        not_implemented("show candidates")
    }
}
