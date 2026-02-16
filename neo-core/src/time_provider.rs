// Copyright (C) 2015-2025 The Neo Project.
//
// time_provider.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

//! Time provider abstraction for testable time-based operations.
//!
//! This module provides a pluggable time source that can be overridden for testing
//! purposes, matching the C# Neo `TimeProvider` behavior.
//!
//! ## Overview
//!
//! The `TimeProvider` allows code to get the current time without directly
//! accessing the system clock, enabling deterministic testing of time-based logic.
//!
//! ## Usage
//!
//! ```rust,no_run
//! use neo_core::time_provider::{TimeProvider, TimeSource};
//! use chrono::Utc;
//!
//! // Get current time
//! let now = TimeProvider::current().utc_now();
//!
//! // For testing, you can override with a fixed time source
//! // TimeProvider::set_current(Arc::new(MyFixedTimeSource));
//! ```

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use std::sync::{Arc, LazyLock};

/// Trait implemented by concrete time sources.
pub trait TimeSource: Send + Sync {
    /// Returns the current UTC time.
    fn utc_now(&self) -> DateTime<Utc>;

    /// Returns the current UTC time as milliseconds since Unix epoch.
    fn utc_now_timestamp_millis(&self) -> i64 {
        datetime_to_millis(self.utc_now())
    }
}

/// Default system-backed time source.
#[derive(Debug, Default)]
struct SystemTimeSource;

impl TimeSource for SystemTimeSource {
    fn utc_now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// Global holder for the currently active time source.
static CURRENT_TIME_SOURCE: LazyLock<RwLock<Arc<dyn TimeSource>>> =
    LazyLock::new(|| RwLock::new(Arc::new(SystemTimeSource) as Arc<dyn TimeSource>));

/// Time provider facade replicating the behaviour of the C# implementation.
#[derive(Debug, Clone, Copy)]
pub struct TimeProvider;

impl TimeProvider {
    /// Returns the currently configured time source.
    pub fn current() -> Arc<dyn TimeSource> {
        CURRENT_TIME_SOURCE.read().clone()
    }

    /// Overrides the currently configured time source.
    pub fn set_current(provider: Arc<dyn TimeSource>) {
        *CURRENT_TIME_SOURCE.write() = provider;
    }

    /// Resets the time source back to the default system implementation.
    pub fn reset_to_default() {
        *CURRENT_TIME_SOURCE.write() = Arc::new(SystemTimeSource);
    }
}

fn datetime_to_millis(datetime: DateTime<Utc>) -> i64 {
    datetime.timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::sync::atomic::{AtomicI64, Ordering};

    #[derive(Debug)]
    struct FixedTimeSource(AtomicI64);

    impl FixedTimeSource {
        fn new(timestamp_millis: i64) -> Self {
            Self(AtomicI64::new(timestamp_millis))
        }
    }

    impl TimeSource for FixedTimeSource {
        fn utc_now(&self) -> DateTime<Utc> {
            let millis = self.0.load(Ordering::Relaxed);
            Utc.timestamp_millis_opt(millis)
                .single()
                .expect("fixed timestamp is representable")
        }
    }

    #[test]
    fn test_time_provider_override() {
        let fixed = Arc::new(FixedTimeSource::new(1_600_000_000_000));
        TimeProvider::set_current(fixed.clone());
        assert_eq!(
            TimeProvider::current().utc_now().timestamp_millis(),
            1_600_000_000_000
        );
        TimeProvider::reset_to_default();
    }
}
