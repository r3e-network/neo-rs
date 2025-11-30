// Copyright (C) 2015-2025 The Neo Project.
//
// log_level.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

/// Represents the level of logs.
/// Matches C# LogLevel enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    /// The debug log level.
    /// Matches C# Debug = DebugLevel
    Debug = 0,

    /// The information log level.
    /// Matches C# Info = InfoLevel
    Info = 1,

    /// The warning log level.
    /// Matches C# Warning = WarningLevel
    Warning = 2,

    /// The error log level.
    /// Matches C# Error = ErrorLevel
    Error = 3,

    /// The fatal log level.
    /// Matches C# Fatal = Error + 1
    Fatal = 4,
}

impl LogLevel {
    /// Gets the debug level
    pub const DEBUG_LEVEL: u8 = 0;

    /// Gets the info level
    pub const INFO_LEVEL: u8 = 1;

    /// Gets the warning level
    pub const WARNING_LEVEL: u8 = 2;

    /// Gets the error level
    pub const ERROR_LEVEL: u8 = 3;

    /// Gets the fatal level
    pub const FATAL_LEVEL: u8 = 4;
}
