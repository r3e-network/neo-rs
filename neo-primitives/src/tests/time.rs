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
