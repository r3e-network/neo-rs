use super::{not_implemented, CommandResult};

/// Native contract management (`MainService.Native`).
pub struct NativeCommands;

impl NativeCommands {
    pub fn list_native_contracts(&self) -> CommandResult {
        not_implemented("list native contracts")
    }
}
