//! Log level enum matching the C#  definition.

/// Represents the severity of a log entry.
/// Mirrors  exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    /// Diagnostic output used for troubleshooting.
    Debug = 0,
    /// Informational message describing normal state transitions.
    Info = 1,
    /// Warning highlighting a potentially problematic condition.
    Warning = 2,
    /// Error indicating a failed operation that allows the node to continue running.
    Error = 3,
    /// Fatal fault indicating the process is about to terminate.
    Fatal = 4,
}

impl LogLevel {
    /// Convenience constant matching the C#  value.
    pub const DEBUG_LEVEL: u8 = 0;
    /// Convenience constant matching the C#  value.
    pub const INFO_LEVEL: u8 = 1;
    /// Convenience constant matching the C#  value.
    pub const WARNING_LEVEL: u8 = 2;
    /// Convenience constant matching the C#  value.
    pub const ERROR_LEVEL: u8 = 3;
    /// Convenience constant matching the C#  value.
    pub const FATAL_LEVEL: u8 = 4;
}

impl From<neo_extensions::LogLevel> for LogLevel {
    fn from(level: neo_extensions::LogLevel) -> Self {
        match level {
            neo_extensions::LogLevel::Debug => LogLevel::Debug,
            neo_extensions::LogLevel::Info => LogLevel::Info,
            neo_extensions::LogLevel::Warning => LogLevel::Warning,
            neo_extensions::LogLevel::Error => LogLevel::Error,
            neo_extensions::LogLevel::Fatal => LogLevel::Fatal,
        }
    }
}
