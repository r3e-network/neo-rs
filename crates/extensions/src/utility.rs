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

/// Log event handler delegate
/// Matches C# LogEventHandler delegate
pub type LogEventHandler = fn(source: String, level: LogLevel, message: String);

/// A utility class that provides common functions.
/// Matches C# Utility class
pub struct Utility;

impl Utility {
    /// Log level property
    /// Matches C# LogLevel property
    pub static LOG_LEVEL: LogLevel = LogLevel::Info;
    
    /// Logging event
    /// Matches C# Logging event
    pub static LOGGING: Option<LogEventHandler> = None;
    
    /// Writes a log.
    /// Matches C# Log method
    pub fn log(source: &str, level: LogLevel, message: &str) {
        if (level as u8) < (Self::LOG_LEVEL as u8) {
            return;
        }
        
        if let Some(handler) = Self::LOGGING {
            handler(source.to_string(), level, message.to_string());
        }
    }
}