use super::CommandResult;
use crate::console_service::ConsoleHelper;
use anyhow::bail;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// Logging configuration commands (`MainService.Logger`).
pub struct LoggerCommands {
    console_enabled: Option<Arc<AtomicBool>>,
}

impl LoggerCommands {
    pub fn new(console_enabled: Option<Arc<AtomicBool>>) -> Self {
        Self { console_enabled }
    }

    pub fn console_log_on(&self) -> CommandResult {
        self.set_console_logging(true)
    }

    pub fn console_log_off(&self) -> CommandResult {
        self.set_console_logging(false)
    }

    fn set_console_logging(&self, enabled: bool) -> CommandResult {
        let Some(flag) = &self.console_enabled else {
            bail!("Logging is disabled; set [logging.active] = true to enable console logging.");
        };
        flag.store(enabled, Ordering::SeqCst);
        if enabled {
            ConsoleHelper::info(["Console logging enabled."]);
        } else {
            ConsoleHelper::info(["Console logging disabled."]);
        }
        Ok(())
    }
}
