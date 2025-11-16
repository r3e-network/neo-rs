use super::{not_implemented, CommandResult};

/// Smart-contract deployment/invocation commands (`MainService.Contracts`).
pub struct ContractCommands;

impl ContractCommands {
    pub fn deploy(&self, _nef_path: &str, _manifest_path: &str) -> CommandResult {
        not_implemented("deploy contract")
    }
}
