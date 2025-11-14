// Copyright (C) 2015-2025 The Neo Project.
//
// utility.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::log_level::LogLevel;
use lazy_static::lazy_static;
use std::sync::Mutex;

/// Log event handler delegate
/// Matches C# LogEventHandler delegate
pub type LogEventHandler = Box<dyn Fn(String, LogLevel, String) + Send + Sync + 'static>;

lazy_static! {
    static ref LOG_LEVEL: Mutex<LogLevel> = Mutex::new(LogLevel::Info);
    static ref LOGGING: Mutex<Option<LogEventHandler>> = Mutex::new(None);
}

/// A utility class that provides common functions.
/// Matches C# Utility class
pub struct Utility;

impl Utility {
    /// Gets the current log level.
    pub fn log_level() -> LogLevel {
        *LOG_LEVEL.lock().unwrap()
    }

    /// Sets the global log level (matches C# setter semantics).
    pub fn set_log_level(level: LogLevel) {
        if let Ok(mut guard) = LOG_LEVEL.lock() {
            *guard = level;
        }
    }

    /// Registers a logging handler.
    pub fn set_logging(handler: Option<LogEventHandler>) {
        if let Ok(mut guard) = LOGGING.lock() {
            *guard = handler;
        }
    }

    /// Writes a log.
    /// Matches C# Log method
    pub fn log(source: &str, level: LogLevel, message: &str) {
        let current_level = Utility::log_level();
        if (level as u8) < (current_level as u8) {
            return;
        }

        if let Ok(handler_guard) = LOGGING.lock() {
            if let Some(handler) = handler_guard.as_ref() {
                handler(source.to_string(), level, message.to_string());
            }
        }
    }
}
