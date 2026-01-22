//! Settings for the ApplicationLogs plugin (mirrors Neo.Plugins.ApplicationLogs).

use crate::unhandled_exception_policy::UnhandledExceptionPolicy;

/// Configuration for application log storage.
#[derive(Debug, Clone)]
pub struct ApplicationLogsSettings {
    /// Whether ApplicationLogs capture is enabled.
    pub enabled: bool,
    /// Network magic number.
    pub network: u32,
    /// Storage path for logs.
    pub path: String,
    /// Maximum stack item size to serialize.
    pub max_stack_size: usize,
    /// Include ApplicationEngine.Log messages.
    pub debug: bool,
    /// Policy for handling unhandled exceptions.
    pub exception_policy: UnhandledExceptionPolicy,
}

impl ApplicationLogsSettings {
    /// Builds settings with explicit parameters.
    pub fn new(
        enabled: bool,
        network: u32,
        path: String,
        max_stack_size: usize,
        debug: bool,
        exception_policy: UnhandledExceptionPolicy,
    ) -> Self {
        Self {
            enabled,
            network,
            path,
            max_stack_size,
            debug,
            exception_policy,
        }
    }
}

impl Default for ApplicationLogsSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            network: 0,
            path: "ApplicationLogs_{0}".to_string(),
            max_stack_size: u16::MAX as usize,
            debug: false,
            exception_policy: UnhandledExceptionPolicy::Ignore,
        }
    }
}
