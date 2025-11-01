// Copyright (C) 2015-2025 The Neo Project.
//
// i_logging_handler.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.
use crate::extensions::log_level::LogLevel;

/// Logging handler interface matching C# ILoggingHandler exactly
pub trait ILoggingHandler {
    /// The handler of Logging event from Utility
    /// Triggered when a new log is added by calling Utility.Log
    /// Matches C# Utility_Logging_Handler method
    fn utility_logging_handler(&self, source: &str, level: LogLevel, message: &str);
}
