use std::any::Any;

/// Trait for handling logging events from the Utility module.
pub trait ILoggingHandler {
    /// Handler for the Utility Logging event.
    ///
    /// This function is triggered when a new log is added by calling `Utility::log()`.
    ///
    /// # Arguments
    ///
    /// * `source` - A string slice that holds the source of the log. Used to identify the producer of the log.
    /// * `level` - The log level, represented by the `LogLevel` enum.
    /// * `message` - The log message, represented as a trait object that can be any type.
    fn utility_logging_handler(&self, source: &str, level: LogLevel, message: &dyn Any);
}
