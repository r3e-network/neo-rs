/// Represents the level of logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    /// The debug log level.
    Debug,

    /// The information log level.
    Info,

    /// The warning log level.
    Warning,

    /// The error log level.
    Error,

    /// The fatal log level.
    Fatal,
}

impl LogLevel {
    pub fn as_byte(&self) -> u8 {
        match self {
            LogLevel::Debug => 0,
            LogLevel::Info => 1,
            LogLevel::Warning => 2,
            LogLevel::Error => 3,
            LogLevel::Fatal => 4,
        }
    }
}
