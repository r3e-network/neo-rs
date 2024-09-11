use std::sync::{Arc, Mutex};
use tokio::sync::Mutex;
use log::{Level, Record};

pub type LogEventHandler = Arc<Mutex<dyn Fn(&str, Level, &str) + Send + Sync>>;

/// A utility module that provides common functions.
pub mod utility {
    use super::*;
    use std::sync::Once;
    use encoding_rs::UTF_8;

    static LOGGING: Once = Once::new();
    static mut LOGGING_HANDLER: Option<LogEventHandler> = None;

    /// A struct representing a logger
    pub struct Logger;

    impl Logger {
        pub fn new() -> Self {
            Logger
        }

        pub async fn initialize(&self) {
            // Initialization logic here
        }

        pub async fn log_event(&self, record: &Record<'_>) {
            log(&record.target(), record.level(), &record.args().to_string()).await;
        }
    }

    /// A strict UTF8 encoding used in NEO system.
    pub fn strict_utf8() -> &'static encoding_rs::Encoding {
        UTF_8
    }

    /// Sets the logging handler
    pub fn set_logging_handler(handler: LogEventHandler) {
        LOGGING.call_once(|| {
            unsafe {
                LOGGING_HANDLER = Some(handler);
            }
        });
    }

    /// Writes a log.
    ///
    /// # Arguments
    ///
    /// * `source` - The source of the log. Used to identify the producer of the log.
    /// * `level` - The level of the log.
    /// * `message` - The message of the log.
    pub async fn log(source: &str, level: Level, message: &str) {
        if let Some(handler) = unsafe { LOGGING_HANDLER.as_ref() } {
            let handler = handler.lock().await;
            handler(source, level, message);
        }
    }
}
