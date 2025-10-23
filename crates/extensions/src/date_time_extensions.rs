// Copyright (C) 2015-2025 The Neo Project.
//
// date_time_extensions.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use std::time::{SystemTime, UNIX_EPOCH};

/// DateTime extensions matching C# DateTimeExtensions exactly
pub trait DateTimeExtensions {
    /// Converts a DateTime to timestamp.
    /// Matches C# ToTimestamp method
    fn to_timestamp(&self) -> u32;

    /// Converts a DateTime to timestamp in milliseconds.
    /// Matches C# ToTimestampMS method
    fn to_timestamp_ms(&self) -> u64;
}

impl DateTimeExtensions for SystemTime {
    fn to_timestamp(&self) -> u32 {
        self.duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as u32
    }

    fn to_timestamp_ms(&self) -> u64 {
        self.duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}
