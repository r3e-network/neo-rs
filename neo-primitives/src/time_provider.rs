// Copyright (C) 2015-2025 The Neo Project.
// Licensed under the MIT License.

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use std::sync::{Arc, LazyLock};

pub trait TimeSource: Send + Sync {
    fn utc_now(&self) -> DateTime<Utc>;

    fn utc_now_timestamp_millis(&self) -> i64 {
        datetime_to_millis(self.utc_now())
    }
}

#[derive(Debug, Default)]
struct SystemTimeSource;

impl TimeSource for SystemTimeSource {
    fn utc_now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

static CURRENT_TIME_SOURCE: LazyLock<RwLock<Arc<dyn TimeSource>>> =
    LazyLock::new(|| RwLock::new(Arc::new(SystemTimeSource) as Arc<dyn TimeSource>));

#[derive(Debug, Clone, Copy)]
pub struct TimeProvider;

impl TimeProvider {
    pub fn current() -> Arc<dyn TimeSource> {
        CURRENT_TIME_SOURCE.read().clone()
    }

    pub fn set_current(provider: Arc<dyn TimeSource>) {
        *CURRENT_TIME_SOURCE.write() = provider;
    }

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
