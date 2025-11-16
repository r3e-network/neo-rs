use super::{not_implemented, CommandResult};

/// Logging configuration commands (`MainService.Logger`).
pub struct LoggerCommands;

impl LoggerCommands {
    pub fn set_log_level(&self, _level: &str) -> CommandResult {
        not_implemented("set loglevel")
    }
}
