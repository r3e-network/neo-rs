use super::{not_implemented, CommandResult};

/// NEP-17 token helpers (`MainService.NEP17`).
pub struct Nep17Commands;

impl Nep17Commands {
    pub fn balance_of(&self, _token: &str, _account: &str) -> CommandResult {
        not_implemented("nep17 balance")
    }
}
